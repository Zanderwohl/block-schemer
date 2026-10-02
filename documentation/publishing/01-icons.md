# Icons

Every icon is made from `crates/block-schemer/assets/logos/lisp-icon-128.png`
with ImageMagick 7 (`magick`; ImageMagick 6 calls it `convert`). Sizes past 128
are upscaled and look soft. For pixel art, add `-filter point` before each
`-resize` to keep hard edges.

## The 128 source

`lisp-icon.png` is 155×130. A centered square crop, scaled to 128:

```sh
magick crates/block-schemer/assets/logos/lisp-icon.png \
  -resize '128x128^' -gravity center -extent 128x128 \
  crates/block-schemer/assets/logos/lisp-icon-128.png
```

`^` fills the square rather than fitting inside it, so the short side becomes
128 and the long side is cropped. To pad instead of crop, use
`-resize 128x128 -background none -gravity center -extent 128x128`.

## Setup

```sh
SRC=crates/block-schemer/assets/logos/lisp-icon-128.png
OUT=crates/block-schemer/assets/icons
mkdir -p "$OUT"
```

## Web

```sh
magick "$SRC" -define icon:auto-resize=48,32,16 "$OUT/favicon.ico"
magick "$SRC" -resize 32x32   "$OUT/favicon-32.png"
magick "$SRC" -resize 180x180 "$OUT/apple-touch-icon.png"
magick "$SRC" -resize 192x192 "$OUT/icon-192.png"
magick "$SRC" -resize 512x512 "$OUT/icon-512.png"
```

```html
<link rel="icon" href="favicon.ico" sizes="any">
<link rel="icon" type="image/png" href="favicon-32.png" sizes="32x32">
<link rel="apple-touch-icon" href="apple-touch-icon.png">
```

`icon-192.png` and `icon-512.png` are for a PWA manifest.

## Windows

```sh
magick "$SRC" -define icon:auto-resize=256,128,64,48,32,24,16 "$OUT/block-schemer.ico"
```

Explorer's large views use the 256 entry; the taskbar and title bar use the
small ones.

## macOS

An `.icns` is built from a folder of PNGs with fixed names. ImageMagick does
not write `.icns` reliably, so it makes the PNGs and Apple's `iconutil` packs
them:

```sh
SET="$OUT/block-schemer.iconset"
mkdir -p "$SET"
for size in 16 32 128 256 512; do
  magick "$SRC" -resize ${size}x${size}         "$SET/icon_${size}x${size}.png"
  magick "$SRC" -resize $((size*2))x$((size*2)) "$SET/icon_${size}x${size}@2x.png"
done
iconutil -c icns "$SET" -o "$OUT/block-schemer.icns"   # macOS only
```

On Linux, `png2icns` from `icnsutils` does the same:

```sh
png2icns "$OUT/block-schemer.icns" "$SET"/icon_{16x16,32x32,128x128,256x256,512x512}.png
```

Since Big Sur, macOS icons sit on a rounded square with a margin. Art that
fills the whole square looks larger than other apps' icons in the Dock; for
about 10% margin, follow each `-resize` in the loop with
`-gravity center -background none -extent` at 1.1 times its size.

## Where they go

- **The running window:** `main` embeds `icon-512.png` (on macOS, the
  iconset's rounded `icon_512x512.png`, since eframe's icon replaces the
  bundle's in the Dock) and passes it as
  `AppConfig::icon`. eframe shows it in the taskbar, the title bar or the
  Dock while the app runs.
- **The Windows executable:** `build.rs` embeds `block-schemer.ico` with
  `winresource` when building on Windows for Windows, so Explorer and
  shortcuts show it. Cross-compiling from another OS skips it.
- **The macOS app:** `cargo install cargo-bundle`, then
  `cargo bundle -p block-schemer --release` makes `Block Schemer.app` from
  `[package.metadata.bundle]` in the crate's `Cargo.toml`, with the iconset
  as its icon.
- **The web version:** the favicon files, once there is one.
