# mc-skin-avatar

Image-level helpers for Minecraft skin textures. It takes PNG bytes and returns PNG bytes — no network, no 3D engine — so it works in any Rust program.

## Features

- Validate a skin and read its pixel size (`64×32` multiples, including HD skins).
- Detect the `slim` (Alex) arm model from the texture.
- Render a flat, square head avatar with the hat layer as an outer shell.

## Install

```toml
[dependencies]
mc-skin-avatar = "0.1"
```

## API

### `dimensions(png: &[u8]) -> Result<(u32, u32), Error>`

Decodes the PNG and checks the size. Width must be a multiple of 64 and height a multiple of 32, so 64×64, legacy 64×32 and HD skins such as 128×128 are accepted.

### `detect_slim(png: &[u8]) -> Result<bool, Error>`

Returns `true` when the texture uses the slim arm model. It checks the left-arm region (`x = 46..48, y = 52..64`, scaled for HD skins); only square skins can be slim.

### `render_avatar(png: &[u8], size: u32) -> Result<Vec<u8>, Error>`

Renders the front of the head as a square `size × size` PNG with transparent corners. The hat is drawn as a slightly larger shell over the face.

### `avatar_png(png: &[u8], size: u32) -> Result<Vec<u8>, Error>`

Simpler variant: crops the 8×8 face, composites the hat on top, and scales with nearest-neighbour for a crisp pixel-art look.

## Example

```rust
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let png = std::fs::read("steve.png")?;

    let (width, height) = mc_skin_avatar::dimensions(&png)?;
    let slim = mc_skin_avatar::detect_slim(&png)?;
    let avatar = mc_skin_avatar::render_avatar(&png, 128)?;

    println!("{width}x{height}, slim = {slim}");
    std::fs::write("avatar.png", avatar)?;
    Ok(())
}
```

## Errors

Every function returns [`mc_skin_avatar::Error`], which implements `std::error::Error`. Invalid PNG data, unsupported dimensions and encode failures are reported there.

## License

MIT
