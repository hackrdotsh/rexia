-- Optional additions to the disposable Rex config for live_copy_mode.
rex.bind("ctrl+alt+shift+c", "client.mode.enter", { name = "copy" })
rex.bind("copy/z", "pane.copy_mode.move", { motion = "right" })
rex.unbind("copy/q")
