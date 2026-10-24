# Wayland only

v1 targets GNOME on Wayland (Ubuntu 26.04 default; the target machine runs Wayland with IBus). Clipboard images, IME, and GL context handling are implemented and tested only for Wayland. X11 may work through GTK but is unsupported, so no X11-specific code paths or tests are added.
