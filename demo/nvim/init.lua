-- Minimal Neovim config for the cypher-lsp demo.
--
-- Usage:
--   cargo build --release -p cyrs-lsp
--   nvim -u demo/nvim/init.lua demo/samples/unclosed_paren.cyp
--
-- No plugins. Just filetype detection, LSP spin-up, and a format-on-save
-- autocommand. Works on Neovim 0.8+ (tested on 0.11).

-- Cypher sources use the .cyp / .cypher extensions.
vim.filetype.add({
  extension = {
    cyp = "cypher",
    cypher = "cypher",
  },
})

-- Locate the cypher-lsp binary. Order of preference:
--   1. $CYPHER_LSP (explicit override)
--   2. `cypher-lsp` on $PATH
--   3. workspace-local target/release/cypher-lsp
--   4. workspace-local target/debug/cypher-lsp
local function find_cypher_lsp()
  if vim.env.CYPHER_LSP and vim.fn.executable(vim.env.CYPHER_LSP) == 1 then
    return vim.env.CYPHER_LSP
  end
  if vim.fn.executable("cypher-lsp") == 1 then
    return "cypher-lsp"
  end
  -- This init.lua lives at <workspace>/demo/nvim/init.lua, so walk up two
  -- levels to find the workspace root and look for build artifacts there.
  local script = debug.getinfo(1, "S").source:sub(2)
  local workspace = vim.fs.dirname(vim.fs.dirname(vim.fs.dirname(script)))
  for _, profile in ipairs({ "release", "debug" }) do
    local candidate = workspace .. "/target/" .. profile .. "/cypher-lsp"
    if vim.fn.executable(candidate) == 1 then
      return candidate
    end
  end
  return nil
end

-- Show diagnostics as virtual text + underline; standard nvim 0.11 defaults.
vim.diagnostic.config({
  virtual_text = true,
  underline = true,
  severity_sort = true,
  float = { border = "rounded" },
})

vim.api.nvim_create_autocmd("FileType", {
  pattern = "cypher",
  callback = function(args)
    local bin = find_cypher_lsp()
    if not bin then
      vim.notify(
        "cypher-lsp binary not found. Build it with:\n"
          .. "    cargo build --release -p cyrs-lsp\n"
          .. "or set $CYPHER_LSP to an absolute path.",
        vim.log.levels.WARN
      )
      return
    end

    -- If $CYPHER_SCHEMA_PATH is set, pass it through as
    -- initializationOptions so the server loads a schema and
    -- surfaces labels + function signatures via completion /
    -- signatureHelp.  Spec §14.3; bead cy-0ls.
    local init_options = nil
    if vim.env.CYPHER_SCHEMA_PATH and vim.env.CYPHER_SCHEMA_PATH ~= "" then
      init_options = {
        schemaSource = "file",
        schemaPath = vim.env.CYPHER_SCHEMA_PATH,
      }
    end

    vim.lsp.start({
      name = "cypher-lsp",
      cmd = { bin },
      init_options = init_options,
      root_dir = vim.fs.root(args.buf, { "Cargo.toml", ".git" })
        or vim.fn.getcwd(),
    }, { bufnr = args.buf })
  end,
})

-- Format-on-save for the buffer's attached cypher-lsp, if any.
vim.api.nvim_create_autocmd("BufWritePre", {
  pattern = { "*.cyp", "*.cypher" },
  callback = function(args)
    vim.lsp.buf.format({ bufnr = args.buf, async = false })
  end,
})

-- Convenience status line so `:messages` isn't the only feedback path.
vim.opt.signcolumn = "yes"
vim.opt.updatetime = 250
vim.api.nvim_create_autocmd({ "CursorHold", "CursorHoldI" }, {
  callback = function()
    vim.diagnostic.open_float(nil, { focus = false, scope = "cursor" })
  end,
})
