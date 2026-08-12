pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: &'static [u8],
}

impl Image {
    pub fn is_intact(&self) -> bool {
        self.rgba.len() == (self.width as usize) * (self.height as usize) * 4
    }
}

macro_rules! image {
    ($name:literal, $side:literal) => {
        Image {
            width: $side,
            height: $side,
            rgba: include_bytes!(concat!("../../assets/", $name, ".rgba")),
        }
    };
}

pub const SPLASH: Image = image!("splash", 512);

pub const NEON_256: Image = image!("icon-neon-256", 256);
pub const NEON_48: Image = image!("icon-neon-48", 48);
pub const NEON_32: Image = image!("icon-neon-32", 32);
pub const NEON_16: Image = image!("icon-neon-16", 16);

pub const TILE_256: Image = image!("icon-tile-256", 256);
pub const TILE_48: Image = image!("icon-tile-48", 48);
pub const TILE_32: Image = image!("icon-tile-32", 32);
pub const TILE_16: Image = image!("icon-tile-16", 16);

pub fn every() -> [&'static Image; 9] {
    [
        &SPLASH, &NEON_256, &NEON_48, &NEON_32, &NEON_16, &TILE_256, &TILE_48, &TILE_32, &TILE_16,
    ]
}

pub fn window_icon() -> &'static Image {
    &NEON_256
}

pub fn tray_icon() -> &'static Image {
    &TILE_32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_image_holds_exactly_the_pixels_its_size_claims() {
        for image in every() {
            assert!(
                image.is_intact(),
                "{}x{} needs {} bytes, has {}",
                image.width,
                image.height,
                image.width as usize * image.height as usize * 4,
                image.rgba.len()
            );
        }
    }

    #[test]
    fn no_image_is_blank() {
        for image in every() {
            let lit = image.rgba.chunks_exact(4).filter(|p| p[3] > 0).count();

            assert!(
                lit * 100 / (image.rgba.len() / 4) > 5,
                "an image that is almost entirely transparent is a broken export"
            );
        }
    }

    #[test]
    fn the_splash_has_transparent_corners_so_it_floats_on_the_desktop() {
        let corner = &SPLASH.rgba[0..4];

        assert_eq!(corner[3], 0, "the splash must not carry a background");
    }

    #[test]
    fn the_tile_icon_is_opaque_in_the_middle_and_cut_away_at_the_corners() {
        let middle = (TILE_32.height / 2 * TILE_32.width + TILE_32.width / 2) as usize * 4;

        assert_eq!(TILE_32.rgba[middle + 3], 255);
        assert_eq!(
            TILE_32.rgba[3], 0,
            "the tile corners are rounded into alpha"
        );
    }

    #[test]
    fn each_role_picks_an_image_that_exists() {
        for image in [window_icon(), tray_icon()] {
            assert!(image.is_intact());
        }
    }
}
