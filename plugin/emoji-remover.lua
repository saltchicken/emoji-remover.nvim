if vim.g.loaded_emoji_remover then
	return
end
vim.g.loaded_emoji_remover = 1

local emoji = require("emoji-remover")

-- Create the user command
vim.api.nvim_create_user_command("EmojiClean", function(opts)
	-- Parse arguments if you want to support passing globs from the command line later
	-- For now, we just run the default configuration
	emoji.clean({})
end, {})
