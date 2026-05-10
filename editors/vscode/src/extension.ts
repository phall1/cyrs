// cyrs VS Code extension — language-client glue for `cypher-lsp`.
//
// The Rust binary (`cypher-lsp`) is feature-complete (hover, definition,
// references, completion + resolve, rename, semantic tokens, inlay hints,
// formatting full + range, code actions, folding, signature help, file
// watchers — see `crates/cyrs-lsp/src/lib.rs::server_capabilities`).
// This extension is a thin client: it spawns the binary on stdio and
// forwards the user's settings as `initializationOptions` (spec §14.3).
//
// Server discovery mirrors `demo/nvim/init.lua`:
//   1. `cyrs.server.path` (workspace setting)
//   2. `$CYPHER_LSP` env var
//   3. `cypher-lsp` on $PATH

import * as path from "node:path";
import { workspace, commands, window, ExtensionContext, OutputChannel } from "vscode";
import {
    LanguageClient,
    LanguageClientOptions,
    ServerOptions,
    TransportKind,
    RevealOutputChannelOn,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let outputChannel: OutputChannel | undefined;

const LANGUAGE_ID = "cypher";
const CLIENT_ID = "cyrs";
const CLIENT_NAME = "cyrs (Cypher / GQL)";

export async function activate(context: ExtensionContext): Promise<void> {
    outputChannel = window.createOutputChannel(CLIENT_NAME);
    context.subscriptions.push(outputChannel);

    context.subscriptions.push(
        commands.registerCommand("cyrs.restartServer", async () => {
            await restart();
        }),
    );

    // Re-launch when the user changes the server path or schema settings.
    context.subscriptions.push(
        workspace.onDidChangeConfiguration((e) => {
            const watched = [
                "cyrs.server.path",
                "cyrs.server.extraEnv",
                "cyrs.schema.source",
                "cyrs.schema.path",
                "cyrs.schema.command",
                "cyrs.dialect",
                "cyrs.watchedFilesDebounceMs",
            ];
            if (watched.some((key) => e.affectsConfiguration(key))) {
                void restart();
            }
        }),
    );

    await start(context);
}

export async function deactivate(): Promise<void> {
    if (client) {
        await client.stop();
        client = undefined;
    }
}

async function start(context: ExtensionContext): Promise<void> {
    const cfg = workspace.getConfiguration(CLIENT_ID);
    const serverPath = resolveServerPath(cfg.get<string>("server.path", ""));

    if (!serverPath) {
        const message =
            "cyrs: `cypher-lsp` binary not found. Set `cyrs.server.path` or build it with " +
            "`cargo build --release -p cyrs-lsp` and ensure it is on $PATH.";
        outputChannel?.appendLine(message);
        void window.showWarningMessage(message);
        return;
    }

    const extraEnv = cfg.get<Record<string, string>>("server.extraEnv", {});
    const env: NodeJS.ProcessEnv = { ...process.env, ...extraEnv };

    const serverOptions: ServerOptions = {
        run: { command: serverPath, transport: TransportKind.stdio, options: { env } },
        debug: { command: serverPath, transport: TransportKind.stdio, options: { env } },
    };

    const clientOptions: LanguageClientOptions = {
        documentSelector: [
            { scheme: "file", language: LANGUAGE_ID },
            { scheme: "untitled", language: LANGUAGE_ID },
        ],
        synchronize: {
            // The server advertises workspace/didChangeWatchedFiles support
            // (spec §14, cy-k2r); register the same globs we treat as
            // Cypher / GQL sources.
            fileEvents: workspace.createFileSystemWatcher(
                "**/*.{cyp,cypher,gql,toml}",
            ),
            configurationSection: CLIENT_ID,
        },
        initializationOptions: buildInitializationOptions(cfg),
        revealOutputChannelOn: RevealOutputChannelOn.Never,
        ...(outputChannel ? { outputChannel } : {}),
    };

    client = new LanguageClient(CLIENT_ID, CLIENT_NAME, serverOptions, clientOptions);
    context.subscriptions.push({ dispose: () => void client?.stop() });

    try {
        await client.start();
        outputChannel?.appendLine(`cypher-lsp started: ${serverPath}`);
    } catch (err) {
        outputChannel?.appendLine(`cypher-lsp failed to start: ${String(err)}`);
        void window.showErrorMessage(
            `cyrs: failed to start cypher-lsp at ${serverPath}. See the cyrs output channel.`,
        );
    }
}

async function restart(): Promise<void> {
    if (!client) {
        return;
    }
    outputChannel?.appendLine("cypher-lsp: restarting…");
    try {
        await client.restart();
    } catch (err) {
        outputChannel?.appendLine(`cypher-lsp restart failed: ${String(err)}`);
        void window.showErrorMessage(`cyrs: restart failed (${String(err)}).`);
    }
}

function resolveServerPath(configured: string): string | undefined {
    const trimmed = configured.trim();
    if (trimmed.length > 0) {
        return path.isAbsolute(trimmed) ? trimmed : trimmed;
    }
    const fromEnv = process.env.CYPHER_LSP;
    if (fromEnv && fromEnv.trim().length > 0) {
        return fromEnv;
    }
    // Fall through to PATH lookup performed by the OS when `cypher-lsp`
    // is spawned by name. vscode-languageclient honours that.
    return "cypher-lsp";
}

interface InitializationOptions {
    schemaSource?: "none" | "file" | "command";
    schemaPath?: string;
    schemaCommand?: string;
    dialect?: "GqlAligned" | "OpenCypherV9";
    watchedFilesDebounceMs?: number;
    formatting?: {
        width: number;
        keywordCasing: "Upper" | "Lower" | "Preserve";
        trailingCommas: "Always" | "AsNeeded" | "Never";
        indentStyle: "Spaces" | "Tabs";
        indentWidth: number;
    };
}

function buildInitializationOptions(
    cfg: ReturnType<typeof workspace.getConfiguration>,
): InitializationOptions {
    const opts: InitializationOptions = {};

    const schemaSource = cfg.get<"none" | "file" | "command">("schema.source", "none");
    if (schemaSource !== "none") {
        opts.schemaSource = schemaSource;
        const schemaPath = cfg.get<string>("schema.path", "").trim();
        if (schemaPath.length > 0) {
            opts.schemaPath = schemaPath;
        }
        const schemaCommand = cfg.get<string>("schema.command", "").trim();
        if (schemaCommand.length > 0) {
            opts.schemaCommand = schemaCommand;
        }
    }

    opts.dialect = cfg.get<"GqlAligned" | "OpenCypherV9">("dialect", "GqlAligned");
    opts.watchedFilesDebounceMs = cfg.get<number>("watchedFilesDebounceMs", 250);

    // The formatting block is a forward-compat slot — `cypher-lsp` does
    // not yet read these from `initializationOptions`, but mirroring the
    // `cyrs-fmt::FormatOptions` struct here lets the user tune it from
    // settings the moment server-side wiring lands. (FYI: the server
    // currently uses `FormatOptions::default()`.)
    opts.formatting = {
        width: cfg.get<number>("formatting.width", 100),
        keywordCasing: cfg.get<"Upper" | "Lower" | "Preserve">(
            "formatting.keywordCasing",
            "Upper",
        ),
        trailingCommas: cfg.get<"Always" | "AsNeeded" | "Never">(
            "formatting.trailingCommas",
            "AsNeeded",
        ),
        indentStyle: cfg.get<"Spaces" | "Tabs">("formatting.indentStyle", "Spaces"),
        indentWidth: cfg.get<number>("formatting.indentWidth", 2),
    };

    return opts;
}
