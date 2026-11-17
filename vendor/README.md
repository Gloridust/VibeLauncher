# Patched dependencies

## softbuffer 0.4.8

`src/backends/wayland/buffer.rs`: `WaylandBuffer::resize` now sets `age = 0`
when the size changes (search for "VibeLauncher patch").

softbuffer keeps two Wayland buffers and reports each one's age, so that a
renderer can redraw only what changed since that buffer was last shown. When the
window size changes, the buffer is recreated at the new size, with a new stride,
over the same memory, but its age was kept. Slint's software renderer then
redrew only the changed areas of the second buffer and left the rest as old
rows read with the new stride. On GNOME with fractional scaling, the window
showed squeezed, repeated and shifted copies of earlier frames.

Remove this folder and the `[patch.crates-io]` entry in the workspace
`Cargo.toml` once softbuffer resets the age itself.
