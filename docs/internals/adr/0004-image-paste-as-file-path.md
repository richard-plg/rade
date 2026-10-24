# Paste images as file paths, not through the agent's clipboard access

On `Ctrl+Shift+V` with an image on the clipboard, RADE saves it to `~/.cache/rade/paste/` and pastes the absolute path. Every supported Agent accepts image paths, while each Agent's own clipboard reading (xclip/wl-paste, OSC 5522) is inconsistent across Agents and terminals. `Ctrl+V` still passes through untouched so an Agent's native clipboard paste keeps working where it exists.
