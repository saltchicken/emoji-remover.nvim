local M = {}

-- Helper to find the plugin root and binary path
local function get_binary_path()
	-- Get the path to this lua file's directory, then go up two levels to root
	local script_path = debug.getinfo(1, "S").source:sub(2)
	local plugin_root = vim.fn.fnamemodify(script_path, ":h:h:h")

	-- The standard location for cargo release builds
	local bin_path = plugin_root .. "/target/release/emoji-remover"

	-- Handle Windows extension if necessary
	if vim.fn.has("win32") == 1 then
		bin_path = bin_path .. ".exe"
	end

	return bin_path
end

function M.clean(opts)
	opts = opts or {}
	local bin = get_binary_path()

	if vim.fn.executable(bin) == 0 then
		vim.notify("Emoji Remover binary not found. Did you run 'cargo build --release'?", vim.log.levels.ERROR)
		return
	end

	-- Save all buffers before running external tools to avoid conflicts
	vim.cmd("wall")

	-- Prepare arguments
	local args = { bin }

	-- Add includes if provided via setup or command
	if opts.include and #opts.include > 0 then
		table.insert(args, "--include")
		for _, pat in ipairs(opts.include) do
			table.insert(args, pat)
		end
	end

	-- Add excludes if provided
	if opts.exclude and #opts.exclude > 0 then
		table.insert(args, "--exclude")
		for _, pat in ipairs(opts.exclude) do
			table.insert(args, pat)
		end
	end

	vim.notify("Running Emoji Remover...", vim.log.levels.INFO)

	-- Run the binary asynchronously
	vim.fn.jobstart(args, {
		on_stdout = function(_, data)
			if data then
				for _, line in ipairs(data) do
					if line ~= "" then
						print(line)
					end
				end
			end
		end,
		on_stderr = function(_, data)
			if data then
				for _, line in ipairs(data) do
					if line ~= "" then
						vim.notify(line, vim.log.levels.WARN)
					end
				end
			end
		end,
		on_exit = function(_, code)
			if code == 0 then
				vim.notify("Emoji removal complete.", vim.log.levels.INFO)
				-- Force Neovim to reload files that changed on disk
				vim.cmd("checktime")
			else
				vim.notify("Emoji removal failed with exit code: " .. code, vim.log.levels.ERROR)
			end
		end,
	})
end

return M
