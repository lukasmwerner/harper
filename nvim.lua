-- This is a script used to debug `harper-ls` in NeoVim.

vim.lsp.start({
  name = "example",
  cmd = vim.lsp.rpc.connect("127.0.0.1", 4000),
  root_dir = "."
})


