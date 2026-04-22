# cypher-lsp demo tape.
#
# Record with:
#   vhs demo/demo.tape
#
# Produces demo/demo.gif. The tape assumes it runs from the workspace
# root. It (re)builds cypher-lsp first so the nvim config can locate
# `target/debug/cypher-lsp`.

Output demo/demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1200
Set Height 700
Set TypingSpeed 50ms
Set Theme "Catppuccin Mocha"

# ---------------------------------------------------------------------
# Build (no-op when already compiled).
# ---------------------------------------------------------------------
Type "cargo build -p cypher-lsp --quiet"
Enter
Sleep 2s

# ---------------------------------------------------------------------
# 1. Parser-recovery diagnostic on unclosed_paren.cyp.
# ---------------------------------------------------------------------
Type "nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp"
Enter
# Wait for LSP to attach and publish diagnostics.
Sleep 4s

# Jump the cursor onto the offending `(` so the floating diagnostic
# surfaces right where the error lives.
Escape
Type "gg"
Sleep 500ms
Type "5j"
Sleep 500ms

# Open the floating diagnostic explicitly — the virtual-text line is
# already visible but the float gives the full E0011 message.
Type ":lua vim.diagnostic.open_float()"
Enter
Sleep 2500ms

# ---------------------------------------------------------------------
# 2. Format-on-save on needs_fmt.cyp.
# ---------------------------------------------------------------------
Escape
Type ":e demo/samples/needs_fmt.cyp"
Enter
# Give the filetype autocmd time to spin up a fresh cypher-lsp client
# for the new buffer before BufWritePre fires.
Sleep 3s

# Trigger the BufWritePre autoformat hook.
Type ":w"
Enter
Sleep 2s

# Pan through the reformatted buffer so the viewer sees the new
# whitespace.
Type "gg"
Sleep 1s

# ---------------------------------------------------------------------
# Exit.
# ---------------------------------------------------------------------
Type ":qa!"
Enter
Sleep 500ms
