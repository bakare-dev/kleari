# Disk image window

`background.png` is the window Finder shows when someone opens Kleari's `.dmg`: drag Kleari
onto Applications, then the first-open steps from [docs/install.md](../../../../docs/install.md)
(Done, never Move to Trash, Open Anyway). `tauri.conf.json` › `bundle.macOS.dmg` points at it
and places the two icons on the arrow.

Edit `background.svg`, then render it (Tauri accepts only PNG, JPG or GIF):

```sh
rsvg-convert -w 660 -h 500 background.svg -o background.png
```

Keep the steps in step with docs/install.md, and keep the window size and icon positions in
`tauri.conf.json` matching the drawing.
