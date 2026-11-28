//! DDS textures as shipped with Fallout: New Vegas (docs/spec-notes/dds.md).
//!
//! Parses the 128-byte header (and the 20-byte DX10 extension after it, when there is
//! one: dds.md §6), names the pixel format, computes the face/mip layout and checks it
//! against the payload size. Block-compressed (DXT/BC) data is meant to be uploaded to
//! the GPU as is; the three uncompressed layouts in the game convert to RGBA8 here, and
//! [`decode_blocks`] decodes DXT1/3/5 on the CPU for the few uses that need the pixels
//! themselves (the mouse pointer); [`decode_bc4`] and [`decode_bc5`] decode the one- and
//! two-channel formats to normalized values. BC7 is never decoded here.

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DdsError {
    #[error("not a DDS file (magic {0:02x?})")]
    BadMagic([u8; 4]),
    #[error("file too short for a DDS header ({0} bytes)")]
    Truncated(usize),
    #[error("unsupported pixel format {0}")]
    Unsupported(String),
    /// Less pixel data than the layout needs (more is taken, [`Dds::parse`]).
    #[error("pixel data is {actual} bytes, the layout needs {expected}")]
    SizeMismatch { expected: usize, actual: usize },
    #[error("size {width}x{height} out of range (1 to {MAX_SIDE})")]
    BadSize { width: u32, height: u32 },
}

/// The largest side [`Dds::parse`] takes. Above every texture the game ships (4096 at
/// most [observed: dds.md §3]) and the engine's own cap (`nvf_assets::texture::MAX_TEXTURE_SIDE`);
/// a larger side, or 0, is a broken header [#1995].
pub const MAX_SIDE: u32 = 16384;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelFormat {
    pub flags: u32,
    pub fourcc: [u8; 4],
    pub rgb_bits: u32,
    /// Red, green, blue, alpha masks.
    pub masks: [u32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Dxt1,
    Dxt3,
    Dxt5,
    /// Legacy ATI2: two independent unsigned normalized channels.
    Bc5U,
    /// Legacy BC5S: two independent signed normalized channels.
    Bc5S,
    /// BC4_UNORM: one unsigned normalized channel (DX10 header or legacy `BC4U`).
    Bc4U,
    /// BC4_SNORM: one signed normalized channel (DX10 header or legacy `BC4S`).
    Bc4S,
    /// BC7_UNORM or BC7_UNORM_SRGB (DX10 header only). The colour space is a hint
    /// ([`DdsHeader::srgb_hint`]), not part of the format.
    Bc7,
    A8R8G8B8,
    R8G8B8,
    A4R4G4B4,
    // Layouts no file of the base game uses (dds.md §2.1): a mod's or a localized
    // install's textures may.
    X8R8G8B8,
    A8B8G8R8,
    X8B8G8R8,
    R5G6B5,
    A1R5G5B5,
    X1R5G5B5,
    L8,
    A8L8,
}

impl Format {
    pub fn name(self) -> &'static str {
        match self {
            Format::Dxt1 => "DXT1",
            Format::Dxt3 => "DXT3",
            Format::Dxt5 => "DXT5",
            Format::Bc5U => "BC5_UNORM",
            Format::Bc5S => "BC5_SNORM",
            Format::Bc4U => "BC4_UNORM",
            Format::Bc4S => "BC4_SNORM",
            Format::Bc7 => "BC7_UNORM",
            Format::A8R8G8B8 => "A8R8G8B8",
            Format::R8G8B8 => "R8G8B8",
            Format::A4R4G4B4 => "A4R4G4B4",
            Format::X8R8G8B8 => "X8R8G8B8",
            Format::A8B8G8R8 => "A8B8G8R8",
            Format::X8B8G8R8 => "X8B8G8R8",
            Format::R5G6B5 => "R5G6B5",
            Format::A1R5G5B5 => "A1R5G5B5",
            Format::X1R5G5B5 => "X1R5G5B5",
            Format::L8 => "L8",
            Format::A8L8 => "A8L8",
        }
    }

    /// Bytes per 4×4 block for block formats: 8 for DXT1/BC1 and BC4, 16 for the others
    /// [documented: MS "Programming Guide for DDS", the pitch of block-compressed formats].
    pub fn block_bytes(self) -> Option<usize> {
        match self {
            Format::Dxt1 | Format::Bc4U | Format::Bc4S => Some(8),
            Format::Dxt3 | Format::Dxt5 | Format::Bc5U | Format::Bc5S | Format::Bc7 => Some(16),
            _ => None,
        }
    }

    /// Bytes per pixel for uncompressed formats decoded by [`to_rgba8`] / [`to_rgba8_rect`],
    /// `None` for block-compressed formats.
    pub fn bytes_per_pixel(self) -> Option<usize> {
        match self {
            Format::A8R8G8B8 | Format::X8R8G8B8 | Format::A8B8G8R8 | Format::X8B8G8R8 => Some(4),
            Format::R8G8B8 => Some(3),
            Format::A4R4G4B4 | Format::R5G6B5 | Format::A1R5G5B5 | Format::X1R5G5B5 | Format::A8L8 => Some(2),
            Format::L8 => Some(1),
            _ => None,
        }
    }

    /// Bytes of one level `w`×`h`, saturating on absurd sizes (a product that would wrap
    /// gave 0, so an empty payload matched a 2-billion-wide header [#1995]).
    pub fn level_size(self, w: u32, h: u32) -> usize {
        let (w, h) = (w as usize, h as usize);
        match self.block_bytes() {
            Some(b) => w.div_ceil(4).max(1).saturating_mul(h.div_ceil(4).max(1)).saturating_mul(b),
            None => w.saturating_mul(h).saturating_mul(self.bytes_per_pixel().unwrap_or(2)),
        }
    }
}

const PF_FOURCC: u32 = 0x4;
const PF_RGB: u32 = 0x40;
const PF_ALPHA_PIXELS: u32 = 0x1;
const PF_LUMINANCE: u32 = 0x20000;
const CAPS2_CUBEMAP: u32 = 0x200;
const CAPS2_ALL_FACES: u32 = 0xFC00;
/// `DDSCAPS2_VOLUME` [documented: MS "DDS_HEADER", dwCaps2].
const CAPS2_VOLUME: u32 = 0x20_0000;
/// `DDS_DIMENSION_TEXTURE1D`, `_TEXTURE2D`, `_TEXTURE3D` [documented: MS
/// "DDS_HEADER_DXT10", resourceDimension].
const DX10_DIMENSION_TEXTURE1D: u32 = 2;
const DX10_DIMENSION_TEXTURE2D: u32 = 3;
const DX10_DIMENSION_TEXTURE3D: u32 = 4;
/// `DDS_RESOURCE_MISC_TEXTURECUBE` [documented: MS "DDS_HEADER_DXT10", miscFlag].
const DX10_MISC_TEXTURECUBE: u32 = 0x4;
/// `DDS_ALPHA_MODE_OPAQUE`, in the low three bits of miscFlags2 [documented: MS
/// "DDS_HEADER_DXT10", miscFlags2].
const DX10_ALPHA_MODE_OPAQUE: u32 = 3;

/// The formats a DX10 header may name that are read here: (DXGI code, format, whether
/// the code is an `_SRGB` one, the DXGI name) [documented: MS "DXGI_FORMAT
/// enumeration", the numbers of `dxgiformat.h`]. R8G8B8A8 is stored R, G, B, A (the
/// A8B8G8R8 layout of dds.md §2.1) and B8G8R8A8 is stored B, G, R, A (A8R8G8B8)
/// [documented: MS "Programming Guide for DDS", the table of common formats].
const DXGI_FORMATS: [(u32, Format, bool, &str); 16] = [
    (28, Format::A8B8G8R8, false, "R8G8B8A8_UNORM"),
    (29, Format::A8B8G8R8, true, "R8G8B8A8_UNORM_SRGB"),
    (71, Format::Dxt1, false, "BC1_UNORM"),
    (72, Format::Dxt1, true, "BC1_UNORM_SRGB"),
    (74, Format::Dxt3, false, "BC2_UNORM"),
    (75, Format::Dxt3, true, "BC2_UNORM_SRGB"),
    (77, Format::Dxt5, false, "BC3_UNORM"),
    (78, Format::Dxt5, true, "BC3_UNORM_SRGB"),
    (80, Format::Bc4U, false, "BC4_UNORM"),
    (81, Format::Bc4S, false, "BC4_SNORM"),
    (83, Format::Bc5U, false, "BC5_UNORM"),
    (84, Format::Bc5S, false, "BC5_SNORM"),
    (87, Format::A8R8G8B8, false, "B8G8R8A8_UNORM"),
    (91, Format::A8R8G8B8, true, "B8G8R8A8_UNORM_SRGB"),
    (98, Format::Bc7, false, "BC7_UNORM"),
    (99, Format::Bc7, true, "BC7_UNORM_SRGB"),
];

fn dxgi(code: u32) -> Option<&'static (u32, Format, bool, &'static str)> {
    DXGI_FORMATS.iter().find(|(c, ..)| *c == code)
}

/// The DXGI name of a code this crate reads (`BC7_UNORM_SRGB`), `None` for any other.
pub fn dxgi_name(code: u32) -> Option<&'static str> {
    dxgi(code).map(|d| d.3)
}

/// The 20-byte extension header that follows the 128-byte header when the pixel format's
/// FourCC is `DX10` [documented: MS "DDS_HEADER_DXT10"].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Dx10Header {
    /// A `DXGI_FORMAT` code ([`dxgi_name`]).
    pub dxgi_format: u32,
    /// 2 = 1D, 3 = 2D, 4 = 3D texture.
    pub resource_dimension: u32,
    /// Bit 0x4: a cube map.
    pub misc_flag: u32,
    /// Textures in the file (cubes, for a cube map).
    pub array_size: u32,
    /// Low three bits: the alpha mode.
    pub misc_flags2: u32,
}

impl Dx10Header {
    pub fn is_cubemap(&self) -> bool {
        self.misc_flag & DX10_MISC_TEXTURECUBE != 0
    }

    /// The alpha mode (0 unknown, 1 straight, 2 premultiplied, 3 opaque, 4 custom).
    pub fn alpha_mode(&self) -> u32 {
        self.misc_flags2 & 7
    }
}

/// Bytes of the DX10 extension header.
pub const DX10_HEADER_LEN: usize = 20;

impl PixelFormat {
    /// Whether a DX10 extension header follows: the FourCC flag set and the FourCC `DX10`
    /// [documented: MS "Programming Guide for DDS", DDS file layout].
    pub fn has_dx10(&self) -> bool {
        self.flags & PF_FOURCC != 0 && &self.fourcc == b"DX10"
    }

    /// The format named in dds.md §2, or a description of an unknown one.
    pub fn format(&self) -> Result<Format, String> {
        // The rows of dds.md §2 match on the whole flags value, except that a FourCC file
        // may also carry the alpha-pixels bit: the FourCC names the format and the bit adds
        // nothing [observed: Honest Hearts' `textures\nvdlc02\landscape\trees\nvdlc02treebark.dds`
        // and Nuclear Sunset's `loading_desktop06.dds`, flags 0x5 with `DXT5`/`DXT1`, which
        // the original draws; they were refused and drew white, log
        // nvforever-20261125-185028-13396.log; hypothesis: the original reads the FourCC
        // whenever its flag is set].
        if self.flags == PF_FOURCC || self.flags == PF_FOURCC | PF_ALPHA_PIXELS {
            return match &self.fourcc {
                b"DXT1" => Ok(Format::Dxt1),
                b"DXT3" => Ok(Format::Dxt3),
                b"DXT5" => Ok(Format::Dxt5),
                b"ATI2" => Ok(Format::Bc5U),
                b"BC5S" => Ok(Format::Bc5S),
                // The legacy codes a reader is to handle for BC4 [documented: MS
                // "Programming Guide for DDS", common formats table]. `ATI1` is not in
                // that table; it is the older code for the same unsigned format
                // [documented: DirectXTex wiki, "DDS I/O Functions", Remarks: `ATI1` and
                // `BC4U` both read as BC4_UNORM; #3151].
                b"BC4U" | b"ATI1" => Ok(Format::Bc4U),
                b"BC4S" => Ok(Format::Bc4S),
                _ => Err(self.describe()),
            };
        }
        if self.flags == PF_RGB || self.flags == PF_RGB | PF_ALPHA_PIXELS {
            let alpha = self.flags & PF_ALPHA_PIXELS != 0;
            return match (self.rgb_bits, self.masks, alpha) {
                (32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000], true) => Ok(Format::A8R8G8B8),
                (24, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, _], false) => Ok(Format::R8G8B8),
                (16, [0x0F00, 0x00F0, 0x000F, 0xF000], true) => Ok(Format::A4R4G4B4),
                (32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, _], false) => Ok(Format::X8R8G8B8),
                (32, [0x0000_00FF, 0x0000_FF00, 0x00FF_0000, 0xFF00_0000], true) => Ok(Format::A8B8G8R8),
                (32, [0x0000_00FF, 0x0000_FF00, 0x00FF_0000, _], false) => Ok(Format::X8B8G8R8),
                (16, [0xF800, 0x07E0, 0x001F, _], false) => Ok(Format::R5G6B5),
                (16, [0x7C00, 0x03E0, 0x001F, 0x8000], true) => Ok(Format::A1R5G5B5),
                (16, [0x7C00, 0x03E0, 0x001F, _], false) => Ok(Format::X1R5G5B5),
                _ => Err(self.describe()),
            };
        }
        if self.flags == PF_LUMINANCE || self.flags == PF_LUMINANCE | PF_ALPHA_PIXELS {
            let alpha = self.flags & PF_ALPHA_PIXELS != 0;
            return match (self.rgb_bits, self.masks[0], self.masks[3], alpha) {
                (8, 0xFF, _, false) => Ok(Format::L8),
                (16, 0xFF, 0xFF00, true) => Ok(Format::A8L8),
                _ => Err(self.describe()),
            };
        }
        Err(self.describe())
    }

    pub fn describe(&self) -> String {
        format!(
            "unknown(flags=0x{:x},fourcc={},bits={},masks={:08x}/{:08x}/{:08x}/{:08x})",
            self.flags,
            String::from_utf8_lossy(&self.fourcc).replace('\0', ""),
            self.rgb_bits,
            self.masks[0],
            self.masks[1],
            self.masks[2],
            self.masks[3]
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DdsHeader {
    pub flags: u32,
    pub height: u32,
    pub width: u32,
    pub pitch_or_linear_size: u32,
    pub depth: u32,
    pub mip_count: u32,
    pub pixel_format: PixelFormat,
    pub caps: [u32; 4],
    /// The DX10 extension header, when the FourCC is `DX10`.
    pub dx10: Option<Dx10Header>,
}

/// Bytes of the magic and the header. The payload starts here unless a DX10 header
/// follows ([`DdsHeader::data_offset`]).
pub const HEADER_LEN: usize = 128;

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

impl DdsHeader {
    pub fn parse(data: &[u8]) -> Result<DdsHeader, DdsError> {
        if data.len() >= 4 && &data[..4] != b"DDS " {
            return Err(DdsError::BadMagic(data[..4].try_into().unwrap()));
        }
        if data.len() < HEADER_LEN {
            return Err(DdsError::Truncated(data.len()));
        }
        let h = |i: usize| u32_at(data, 4 + 4 * i);
        let pixel_format = PixelFormat {
            flags: u32_at(data, 80),
            fourcc: data[84..88].try_into().unwrap(),
            rgb_bits: u32_at(data, 88),
            masks: [u32_at(data, 92), u32_at(data, 96), u32_at(data, 100), u32_at(data, 104)],
        };
        // A DX10 file is at least 148 bytes [documented: MS "Programming Guide for DDS",
        // DDS variants].
        let dx10 = if pixel_format.has_dx10() {
            if data.len() < HEADER_LEN + DX10_HEADER_LEN {
                return Err(DdsError::Truncated(data.len()));
            }
            let x = |i: usize| u32_at(data, HEADER_LEN + 4 * i);
            Some(Dx10Header { dxgi_format: x(0), resource_dimension: x(1), misc_flag: x(2), array_size: x(3), misc_flags2: x(4) })
        } else {
            None
        };
        Ok(DdsHeader {
            flags: h(1),
            height: h(2),
            width: h(3),
            pitch_or_linear_size: h(4),
            depth: h(5),
            mip_count: h(6),
            pixel_format,
            caps: [u32_at(data, 108), u32_at(data, 112), u32_at(data, 116), u32_at(data, 120)],
            dx10,
        })
    }

    /// Where the pixel data starts: after the DX10 header when there is one (148), else
    /// right after the header (128) [documented: MS "Programming Guide for DDS"].
    pub fn data_offset(&self) -> usize {
        if self.dx10.is_some() { HEADER_LEN + DX10_HEADER_LEN } else { HEADER_LEN }
    }

    /// The format: by the DXGI code of a DX10 header (the table above), else by the
    /// legacy pixel format (dds.md §2); a description of an unknown one.
    pub fn format(&self) -> Result<Format, String> {
        match self.dx10 {
            Some(x) => dxgi(x.dxgi_format).map(|d| d.1).ok_or_else(|| format!("unknown(dxgi={})", x.dxgi_format)),
            None => self.pixel_format.format(),
        }
    }

    /// The format's name as the file states it: the DXGI name for a DX10 header
    /// (`BC7_UNORM_SRGB`), else the legacy name ([`Format::name`]).
    pub fn format_name(&self) -> Result<&'static str, String> {
        match self.dx10 {
            Some(x) => dxgi_name(x.dxgi_format).ok_or_else(|| format!("unknown(dxgi={})", x.dxgi_format)),
            None => self.pixel_format.format().map(Format::name),
        }
    }

    /// Whether the file says its colours are sRGB: `Some(true)` for an `_SRGB` DXGI code,
    /// `Some(false)` for another known one, `None` without a DX10 header (legacy files
    /// say nothing). A hint only: the engine picks the colour space by the texture's role.
    pub fn srgb_hint(&self) -> Option<bool> {
        self.dx10.and_then(|x| dxgi(x.dxgi_format)).map(|d| d.2)
    }

    /// Whether a DX10 header states that every alpha is opaque (`DDS_ALPHA_MODE_OPAQUE`).
    pub fn alpha_opaque(&self) -> bool {
        self.dx10.is_some_and(|x| x.alpha_mode() == DX10_ALPHA_MODE_OPAQUE)
    }

    /// Why a DX10 header describes something other than one flat 2D texture, if it does:
    /// a 1D or 3D texture, a cube map or an array. Only single 2D textures are read.
    pub fn dx10_refusal(&self) -> Option<String> {
        let x = self.dx10?;
        if x.resource_dimension != DX10_DIMENSION_TEXTURE2D {
            Some(format!("DX10 resource dimension {} (only 2D textures are read)", x.resource_dimension))
        } else if self.is_cubemap() {
            Some("DX10 cube map (not read)".into())
        } else if x.array_size != 1 {
            Some(format!("DX10 array of {} textures (only single textures are read)", x.array_size))
        } else {
            None
        }
    }

    /// Mip levels in the file: the stored count, or 1 when it is 0 (dds.md §3).
    pub fn levels(&self) -> u32 {
        self.mip_count.max(1)
    }

    /// A cube map: the legacy capability bit, or a DX10 header's cube flag.
    pub fn is_cubemap(&self) -> bool {
        self.caps[1] & CAPS2_CUBEMAP != 0 || self.dx10.is_some_and(|x| x.is_cubemap())
    }

    /// Faces stored per array element: six for a DX10 cube map, each element of its array
    /// being one cube [documented: MS "DDS_HEADER_DXT10", arraySize]; the legacy face
    /// bits otherwise. [`Dds::parse`] refuses DX10 cube maps anyway.
    pub fn faces(&self) -> u32 {
        if self.dx10.is_some_and(|x| x.is_cubemap()) {
            6
        } else if self.is_cubemap() {
            (self.caps[1] & CAPS2_ALL_FACES).count_ones().max(1)
        } else {
            1
        }
    }

    /// (width, height) of mip level `i`.
    pub fn level_dims(&self, i: u32) -> (u32, u32) {
        (self.width.checked_shr(i).unwrap_or(0).max(1), self.height.checked_shr(i).unwrap_or(0).max(1))
    }

    /// Array elements stored: a DX10 header's array size (cubes, for a cube map), else 1.
    pub fn array_elements(&self) -> u32 {
        self.dx10.map_or(1, |x| x.array_size)
    }

    /// Whether a DX10 volume texture: each level then holds `depth` slices, halving like
    /// the sides with a minimum of 1, all slices of a level together [documented: MS
    /// "DDS_HEADER_DXT10", resourceDimension; MS "DDS Volume Texture Example"]. A legacy
    /// volume is not sized (no game file is a volume, dds.md §3).
    fn is_dx10_volume(&self) -> bool {
        self.dx10.is_some_and(|x| x.resource_dimension == DX10_DIMENSION_TEXTURE3D)
    }

    /// A volume texture: the legacy capability bit (`DDSCAPS2_VOLUME`) [documented: MS
    /// "DDS_HEADER", dwCaps2], or a DX10 header's 3D dimension.
    pub fn is_volume(&self) -> bool {
        self.caps[1] & CAPS2_VOLUME != 0 || self.is_dx10_volume()
    }

    /// Whether [`Self::expected_size`] sizes this layout: a legacy file other than a
    /// volume, and a DX10 one of 1D or 2D textures (an array of at least one; cubes 2D
    /// only) or a single volume. Anything else (a legacy volume, an unknown dimension, an
    /// array of 0, a volume array or cube, a 1D cube) is not sized: the listing prints
    /// `null`. The cube test is the listing's own (`is_cubemap`: the DX10 flag or the
    /// legacy bit).
    pub fn layout_sized(&self) -> bool {
        match self.dx10 {
            None => !self.is_volume(),
            Some(x) => match x.resource_dimension {
                DX10_DIMENSION_TEXTURE1D => x.array_size >= 1 && !self.is_cubemap(),
                DX10_DIMENSION_TEXTURE2D => x.array_size >= 1,
                DX10_DIMENSION_TEXTURE3D => x.array_size == 1 && !self.is_cubemap(),
                _ => false,
            },
        }
    }

    /// Payload bytes the layout needs (all array elements, faces, levels and, for a DX10
    /// volume, slices), saturating on absurd headers. Meaningful when
    /// [`Self::layout_sized`].
    pub fn expected_size(&self, f: Format) -> usize {
        let volume = self.is_dx10_volume();
        let per_face = (0..self.levels().min(32)).fold(0usize, |acc, i| {
            let (w, h) = self.level_dims(i);
            let slices = if volume { self.depth.checked_shr(i).unwrap_or(0).max(1) as usize } else { 1 };
            acc.saturating_add(f.level_size(w, h).saturating_mul(slices))
        });
        per_face.saturating_mul(self.faces() as usize).saturating_mul(self.array_elements() as usize)
    }
}

/// A parsed texture with its level slices.
#[derive(Debug, Clone)]
pub struct Dds<'a> {
    pub header: DdsHeader,
    pub format: Format,
    /// `levels[face][mip]` slices of the payload.
    pub levels: Vec<Vec<&'a [u8]>>,
}

impl<'a> Dds<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Dds<'a>, DdsError> {
        let header = DdsHeader::parse(data)?;
        let format = header.format().map_err(DdsError::Unsupported)?;
        if let Some(why) = header.dx10_refusal() {
            return Err(DdsError::Unsupported(why));
        }
        // A side of 0 laid out as 1 wide while the header kept 0; one past the cap gave
        // callers that size the image from the header a 2-billion-wide texture [#1995].
        if !(1..=MAX_SIDE).contains(&header.width) || !(1..=MAX_SIDE).contains(&header.height) {
            return Err(DdsError::BadSize { width: header.width, height: header.height });
        }
        let payload = &data[header.data_offset()..];
        let expected = header.expected_size(format);
        // More data than the layout needs is taken: the levels the header counts are
        // read and the rest is left alone (dds.md §3, #2164). Less is a cut-off file.
        if payload.len() < expected || header.levels() > 32 {
            return Err(DdsError::SizeMismatch { expected, actual: payload.len() });
        }
        let mut levels = Vec::new();
        let mut off = 0;
        for _ in 0..header.faces() {
            let mut face = Vec::new();
            for i in 0..header.levels() {
                let (w, h) = header.level_dims(i);
                let n = format.level_size(w, h);
                face.push(&payload[off..off + n]);
                off += n;
            }
            levels.push(face);
        }
        Ok(Dds { header, format, levels })
    }

    /// [`DdsHeader::srgb_hint`].
    pub fn srgb_hint(&self) -> Option<bool> {
        self.header.srgb_hint()
    }

    /// [`DdsHeader::alpha_opaque`].
    pub fn alpha_opaque(&self) -> bool {
        self.header.alpha_opaque()
    }
}

fn append_rgba8(format: Format, src: &[u8], out: &mut Vec<u8>) -> bool {
    match format {
        Format::A8R8G8B8 => {
            for p in src.as_chunks::<4>().0 {
                out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
            }
        }
        Format::R8G8B8 => {
            for p in src.as_chunks::<3>().0 {
                out.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        Format::A4R4G4B4 => {
            for p in src.as_chunks::<2>().0 {
                let v = u16::from_le_bytes([p[0], p[1]]);
                let ch = |shift: u16| (((v >> shift) & 0xF) as u8) * 17;
                out.extend_from_slice(&[ch(8), ch(4), ch(0), ch(12)]);
            }
        }
        Format::X8R8G8B8 => {
            for p in src.as_chunks::<4>().0 {
                out.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        Format::A8B8G8R8 => out.extend_from_slice(src),
        Format::X8B8G8R8 => {
            for p in src.as_chunks::<4>().0 {
                out.extend_from_slice(&[p[0], p[1], p[2], 255]);
            }
        }
        Format::R5G6B5 => {
            for p in src.as_chunks::<2>().0 {
                let [r, g, b] = rgb565(u16::from_le_bytes([p[0], p[1]]));
                out.extend_from_slice(&[r, g, b, 255]);
            }
        }
        Format::A1R5G5B5 | Format::X1R5G5B5 => {
            for p in src.as_chunks::<2>().0 {
                let v = u16::from_le_bytes([p[0], p[1]]);
                let ch = |shift: u16| {
                    let c = ((v >> shift) & 31) as u8;
                    (c << 3) | (c >> 2)
                };
                let a = if format == Format::X1R5G5B5 || v & 0x8000 != 0 { 255 } else { 0 };
                out.extend_from_slice(&[ch(10), ch(5), ch(0), a]);
            }
        }
        // Luminance is the grey of all three colours, alpha 1 unless stored [documented:
        // MS "Map Direct3D 9 Formats to Direct3D 10", L8 and A8L8 read as R8G8B8A8 with
        // the grey copied].
        Format::L8 => {
            for &l in src {
                out.extend_from_slice(&[l, l, l, 255]);
            }
        }
        Format::A8L8 => {
            for p in src.as_chunks::<2>().0 {
                out.extend_from_slice(&[p[0], p[0], p[0], p[1]]);
            }
        }
        _ => return false,
    }
    true
}

/// Convert one level of an uncompressed format to RGBA8. `None` for block formats.
pub fn to_rgba8(format: Format, src: &[u8]) -> Option<Vec<u8>> {
    let bpp = format.bytes_per_pixel()?;
    let mut out = Vec::with_capacity(src.len() / bpp * 4);
    append_rgba8(format, src, &mut out).then_some(out)
}

/// Convert a sub-rectangle `rect` (`[x, y, width, height]`) of one uncompressed level of
/// `w`×`h` texels to RGBA8 without allocating or decoding the rest of the level [design].
/// `None` for block formats, invalid dimensions or bounds, or data too short.
pub fn to_rgba8_rect(format: Format, w: u32, h: u32, src: &[u8], rect: [u32; 4]) -> Option<Vec<u8>> {
    let bpp = format.bytes_per_pixel()?;
    let [rx, ry, rw, rh] = rect;
    if !(1..=MAX_SIDE).contains(&w)
        || !(1..=MAX_SIDE).contains(&h)
        || rw == 0
        || rh == 0
        || rx.checked_add(rw).is_none_or(|e| e > w)
        || ry.checked_add(rh).is_none_or(|e| e > h)
        || src.len() < format.level_size(w, h)
    {
        return None;
    }
    let (w, rx, ry, rw, rh) = (w as usize, rx as usize, ry as usize, rw as usize, rh as usize);
    let mut out = Vec::new();
    out.try_reserve_exact(rw.checked_mul(rh)?.checked_mul(4)?).ok()?;
    let row_bytes = rw * bpp;
    for y in ry..ry + rh {
        let off = (y * w + rx) * bpp;
        if !append_rgba8(format, &src[off..off + row_bytes], &mut out) {
            return None;
        }
    }
    Some(out)
}

/// A colour as RGB565, widened to 8 bits a channel.
fn rgb565(v: u16) -> [u8; 3] {
    let (r, g, b) = ((v >> 11) & 31, (v >> 5) & 63, v & 31);
    [((r << 3) | (r >> 2)) as u8, ((g << 2) | (g >> 4)) as u8, ((b << 3) | (b >> 2)) as u8]
}

/// The 16 colours of a DXT colour block (8 bytes), alpha 255 except DXT1's transparent
/// entry. `four_always`: DXT3/5 always use the four-colour mode [documented: Microsoft,
/// "Block Compression (Direct3D 10)", BC1–BC3].
fn color_block(b: &[u8], four_always: bool) -> [[u8; 4]; 16] {
    let (c0, c1) = (u16::from_le_bytes([b[0], b[1]]), u16::from_le_bytes([b[2], b[3]]));
    let (p0, p1) = (rgb565(c0), rgb565(c1));
    let mix = |a: u8, b: u8, wa: u16, wb: u16| ((u16::from(a) * wa + u16::from(b) * wb) / (wa + wb)) as u8;
    let mut pal = [[0u8; 4]; 4];
    pal[0] = [p0[0], p0[1], p0[2], 255];
    pal[1] = [p1[0], p1[1], p1[2], 255];
    if c0 > c1 || four_always {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 2, 1);
            pal[3][k] = mix(p0[k], p1[k], 1, 2);
        }
        pal[2][3] = 255;
        pal[3][3] = 255;
    } else {
        for k in 0..3 {
            pal[2][k] = mix(p0[k], p1[k], 1, 1);
        }
        pal[2][3] = 255;
        pal[3] = [0, 0, 0, 0];
    }
    let bits = u32::from_le_bytes([b[4], b[5], b[6], b[7]]);
    std::array::from_fn(|i| pal[((bits >> (2 * i)) & 3) as usize])
}

/// The 16 alphas of a DXT5 alpha block (8 bytes).
fn alpha_block5(b: &[u8]) -> [u8; 16] {
    let (a0, a1) = (u16::from(b[0]), u16::from(b[1]));
    let mut pal = [0u8; 8];
    pal[0] = a0 as u8;
    pal[1] = a1 as u8;
    if a0 > a1 {
        for i in 1..7u16 {
            pal[i as usize + 1] = ((a0 * (7 - i) + a1 * i) / 7) as u8;
        }
    } else {
        for i in 1..5u16 {
            pal[i as usize + 1] = ((a0 * (5 - i) + a1 * i) / 5) as u8;
        }
        pal[6] = 0;
        pal[7] = 255;
    }
    let bits = b[2..8].iter().rev().fold(0u64, |acc, &x| (acc << 8) | u64::from(x));
    std::array::from_fn(|i| pal[((bits >> (3 * i)) & 7) as usize])
}

/// The normalized values of one BC4 channel (dds-bc5.md). Unlike DXT5 alpha,
/// interpolate floating-point endpoints rather than rounding the palette to bytes.
fn bc5_channel(block: &[u8], signed: bool) -> [f32; 16] {
    let (a, b, six) = if signed {
        let (a, b) = (block[0] as i8, block[1] as i8);
        ((f32::from(a) / 127.0).max(-1.0), (f32::from(b) / 127.0).max(-1.0), a > b)
    } else {
        (f32::from(block[0]) / 255.0, f32::from(block[1]) / 255.0, block[0] > block[1])
    };
    let mut palette = [0.0; 8];
    palette[0] = a;
    palette[1] = b;
    let denominator = if six { 7 } else { 5 };
    for k in 1..denominator {
        palette[k + 1] = (a * (denominator - k) as f32 + b * k as f32) / denominator as f32;
    }
    if !six {
        palette[6] = if signed { -1.0 } else { 0.0 };
        palette[7] = 1.0;
    }
    let bits = block[2..8].iter().enumerate().fold(0u64, |bits, (i, &byte)| bits | (u64::from(byte) << (i * 8)));
    std::array::from_fn(|i| palette[((bits >> (i * 3)) & 7) as usize])
}

/// Decode one BC4 level to row-major normalized values: BC4_SNORM in [-1, 1], BC4_UNORM
/// in [0, 1]. A BC4 block has the layout of one BC5 channel [documented: MS "Block
/// Compression (Direct3D 10)", BC4 and BC5], so it decodes as dds-bc5.md says. Refuses
/// other formats, invalid dimensions and truncated blocks before allocation.
pub fn decode_bc4(format: Format, w: u32, h: u32, src: &[u8]) -> Option<Vec<f32>> {
    if !matches!(format, Format::Bc4U | Format::Bc4S)
        || !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h)
        || src.len() < format.level_size(w, h)
    {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let across = w.div_ceil(4);
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(w.checked_mul(h)?).ok()?;
    pixels.resize(w * h, 0.0);
    for by in 0..h.div_ceil(4) {
        for bx in 0..across {
            let block = &src[(by * across + bx) * 8..][..8];
            for (i, v) in bc5_channel(block, format == Format::Bc4S).into_iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    pixels[y * w + x] = v;
                }
            }
        }
    }
    Some(pixels)
}

/// Decode one legacy BC5 level to row-major normalized red/green values.
/// Signed BC5S stays in [-1, 1], unsigned BC5U in [0, 1]. This is data, not a
/// normal-map conversion: no bias, Z reconstruction, green flip, or color space.
/// Refuses other formats, invalid dimensions and truncated blocks before allocation.
pub fn decode_bc5(format: Format, w: u32, h: u32, src: &[u8]) -> Option<Vec<[f32; 2]>> {
    if !matches!(format, Format::Bc5U | Format::Bc5S)
        || !(1..=MAX_SIDE).contains(&w) || !(1..=MAX_SIDE).contains(&h)
        || src.len() < format.level_size(w, h)
    {
        return None;
    }
    let (w, h) = (w as usize, h as usize);
    let across = w.div_ceil(4);
    let mut pixels = Vec::new();
    pixels.try_reserve_exact(w.checked_mul(h)?).ok()?;
    pixels.resize(w * h, [0.0; 2]);
    for by in 0..h.div_ceil(4) {
        for bx in 0..across {
            let block = &src[(by * across + bx) * 16..][..16];
            let red = bc5_channel(&block[..8], format == Format::Bc5S);
            let green = bc5_channel(&block[8..], format == Format::Bc5S);
            for i in 0..16 {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x < w && y < h {
                    pixels[y * w + x] = [red[i], green[i]];
                }
            }
        }
    }
    Some(pixels)
}

fn decode_dxt_block(format: Format, b: &[u8]) -> [[u8; 4]; 16] {
    match format {
        Format::Dxt1 => color_block(b, false),
        Format::Dxt3 => {
            let mut c = color_block(&b[8..], true);
            for (i, t) in c.iter_mut().enumerate() {
                let nib = (b[i / 2] >> (4 * (i % 2))) & 15;
                t[3] = nib * 17;
            }
            c
        }
        _ => {
            let a = alpha_block5(b);
            let mut c = color_block(&b[8..], true);
            for (t, a) in c.iter_mut().zip(a) {
                t[3] = a;
            }
            c
        }
    }
}

/// Decode one level of DXT1/3/5 data of `w`×`h` texels to RGBA8 (row-major); `None` for
/// other formats, data too short, or a side of 0 or above [`MAX_SIDE`] (every caller's
/// size comes from a parsed header, which is within those bounds), checked before
/// anything is allocated.
pub fn decode_blocks(format: Format, w: u32, h: u32, src: &[u8]) -> Option<Vec<u8>> {
    decode_blocks_rect(format, w, h, src, [0, 0, w, h])
}

/// Decode a sub-rectangle `rect` (`[x, y, width, height]`) of one level of DXT1/3/5 data
/// of `w`×`h` texels to RGBA8 (row-major), touching only the 4×4 blocks overlapping `rect`
/// [design]. `None` for other formats, data too short, invalid dimensions, or an empty or
/// out-of-bounds rectangle, checked before anything is allocated.
pub fn decode_blocks_rect(format: Format, w: u32, h: u32, src: &[u8], rect: [u32; 4]) -> Option<Vec<u8>> {
    let block = format.block_bytes().filter(|_| matches!(format, Format::Dxt1 | Format::Dxt3 | Format::Dxt5))?;
    let [rx, ry, rw, rh] = rect;
    if !(1..=MAX_SIDE).contains(&w)
        || !(1..=MAX_SIDE).contains(&h)
        || rw == 0
        || rh == 0
        || rx.checked_add(rw).is_none_or(|e| e > w)
        || ry.checked_add(rh).is_none_or(|e| e > h)
        || src.len() < format.level_size(w, h)
    {
        return None;
    }
    let bw = (w as usize).div_ceil(4);
    let (rx, ry, rw, rh) = (rx as usize, ry as usize, rw as usize, rh as usize);
    let mut out = Vec::new();
    out.try_reserve_exact(rw.checked_mul(rh)?.checked_mul(4)?).ok()?;
    out.resize(rw * rh * 4, 0u8);
    let (bx0, bx1) = (rx / 4, (rx + rw).div_ceil(4));
    let (by0, by1) = (ry / 4, (ry + rh).div_ceil(4));
    for by in by0..by1 {
        for bx in bx0..bx1 {
            let b = &src[(by * bw + bx) * block..][..block];
            let texels = decode_dxt_block(format, b);
            for (i, t) in texels.iter().enumerate() {
                let (x, y) = (bx * 4 + i % 4, by * 4 + i / 4);
                if x >= rx && x < rx + rw && y >= ry && y < ry + rh {
                    out[((y - ry) * rw + (x - rx)) * 4..][..4].copy_from_slice(t);
                }
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dxt_blocks_decode_to_their_colours_and_alphas() {
        // DXT1: c0 white (0xFFFF) > c1 black: indices 0,1,2,3 across the first row.
        let b = [0xFF, 0xFF, 0x00, 0x00, 0b11_10_01_00, 0, 0, 0];
        let px = decode_blocks(Format::Dxt1, 4, 4, &b).unwrap();
        assert_eq!(&px[0..16], &[255, 255, 255, 255, 0, 0, 0, 255, 170, 170, 170, 255, 85, 85, 85, 255]);
        // c0 <= c1: index 3 is transparent.
        let b = [0x00, 0x00, 0xFF, 0xFF, 0b11, 0, 0, 0];
        assert_eq!(&decode_blocks(Format::Dxt1, 4, 4, &b).unwrap()[0..4], &[0, 0, 0, 0]);
        // DXT5: alpha 255 / 0 with index 1 (0) on texel 0, index 0 (255) elsewhere.
        let mut b = [0u8; 16];
        b[0] = 255;
        b[1] = 0;
        b[2] = 1;
        b[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let px = decode_blocks(Format::Dxt5, 4, 4, &b).unwrap();
        assert_eq!((px[3], px[7]), (0, 255));
        // DXT3: explicit 4-bit alphas.
        let mut b = [0u8; 16];
        b[0] = 0xF0;
        let px = decode_blocks(Format::Dxt3, 4, 4, &b).unwrap();
        assert_eq!((px[3], px[7]), (0, 255));
        assert!(decode_blocks(Format::A8R8G8B8, 4, 4, &b).is_none());
    }

    #[allow(clippy::too_many_arguments)]
    fn build(w: u32, h: u32, mips: u32, pf_flags: u32, fourcc: &[u8; 4], bits: u32, masks: [u32; 4], caps2: u32, payload: usize) -> Vec<u8> {
        let mut d = vec![0u8; HEADER_LEN];
        d[..4].copy_from_slice(b"DDS ");
        let put = |d: &mut Vec<u8>, o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
        put(&mut d, 4, 124);
        put(&mut d, 8, 0x1007 | 0x20000);
        put(&mut d, 12, h);
        put(&mut d, 16, w);
        put(&mut d, 28, mips);
        put(&mut d, 76, 32);
        put(&mut d, 80, pf_flags);
        d[84..88].copy_from_slice(fourcc);
        put(&mut d, 88, bits);
        for (k, m) in masks.iter().enumerate() {
            put(&mut d, 92 + 4 * k, *m);
        }
        put(&mut d, 112, caps2);
        d.resize(HEADER_LEN + payload, 0xAB);
        d
    }

    #[test]
    fn dxt1_chain() {
        // 8×4: levels 8×4 (2 blocks), 4×2 (1), 2×1 (1), 1×1 (1) = 5 blocks × 8 bytes.
        let d = build(8, 4, 4, PF_FOURCC, b"DXT1", 0, [0; 4], 0, 40);
        let t = Dds::parse(&d).unwrap();
        assert_eq!(t.format, Format::Dxt1);
        assert_eq!(t.levels[0].iter().map(|l| l.len()).collect::<Vec<_>>(), [16, 8, 8, 8]);
        let short = build(8, 4, 4, PF_FOURCC, b"DXT1", 0, [0; 4], 0, 39);
        assert_eq!(Dds::parse(&short).unwrap_err(), DdsError::SizeMismatch { expected: 40, actual: 39 });
    }

    /// Bytes after the last level the header counts are left alone: a mod's DXT1 with
    /// one block more in the file than its counted levels need loads those levels [#2164].
    #[test]
    fn data_after_the_last_level_is_ignored() {
        let d = build(8, 4, 3, PF_FOURCC, b"DXT1", 0, [0; 4], 0, 40);
        let t = Dds::parse(&d).unwrap();
        assert_eq!(t.levels[0].iter().map(|l| l.len()).collect::<Vec<_>>(), [16, 8, 8]);
        let argb = build(2, 1, 1, PF_RGB | PF_ALPHA_PIXELS, &[0; 4], 32, [0xFF0000, 0xFF00, 0xFF, 0xFF000000], 0, 11);
        assert_eq!(Dds::parse(&argb).unwrap().levels[0][0].len(), 8);
    }

    #[test]
    fn a_fourcc_with_the_alpha_pixels_bit_reads_as_its_fourcc() {
        for (cc, f) in [(b"DXT5", Format::Dxt5), (b"DXT1", Format::Dxt1)] {
            let d = build(4, 4, 1, PF_FOURCC | PF_ALPHA_PIXELS, cc, 0, [0; 4], 0, if f == Format::Dxt1 { 8 } else { 16 });
            assert_eq!(Dds::parse(&d).unwrap().header.pixel_format.format(), Ok(f));
        }
    }

    #[test]
    fn zero_mip_count_means_one_level() {
        let d = build(4, 4, 0, PF_FOURCC, b"DXT5", 0, [0; 4], 0, 16);
        assert_eq!(Dds::parse(&d).unwrap().levels[0].len(), 1);
    }

    #[test]
    fn cubemap_faces() {
        let d = build(4, 4, 1, PF_FOURCC, b"DXT3", 0, [0; 4], CAPS2_CUBEMAP | CAPS2_ALL_FACES, 6 * 16);
        let t = Dds::parse(&d).unwrap();
        assert_eq!(t.header.faces(), 6);
        assert_eq!(t.levels.len(), 6);
    }

    #[test]
    fn uncompressed_formats() {
        let argb = build(1, 1, 1, PF_RGB | PF_ALPHA_PIXELS, &[0; 4], 32, [0xFF0000, 0xFF00, 0xFF, 0xFF000000], 0, 4);
        assert_eq!(Dds::parse(&argb).unwrap().format, Format::A8R8G8B8);
        assert_eq!(to_rgba8(Format::A8R8G8B8, &[1, 2, 3, 4]).unwrap(), [3, 2, 1, 4]);
        assert_eq!(to_rgba8(Format::R8G8B8, &[1, 2, 3]).unwrap(), [3, 2, 1, 255]);
        assert_eq!(to_rgba8(Format::A4R4G4B4, &[0x21, 0xF3]).unwrap(), [0x33, 0x22, 0x11, 0xFF]);
        let odd = build(1, 1, 1, PF_RGB, &[0; 4], 16, [0x1F, 0x7E0, 0xF800, 0], 0, 2);
        assert!(matches!(Dds::parse(&odd), Err(DdsError::Unsupported(_))));
    }

    #[test]
    fn layouts_beyond_the_base_games() {
        let fmt = |flags, bits, masks| Dds::parse(&build(1, 1, 1, flags, &[0; 4], bits, masks, 0, (bits / 8) as usize)).map(|d| d.format);
        assert_eq!(fmt(PF_RGB, 32, [0xFF0000, 0xFF00, 0xFF, 0]), Ok(Format::X8R8G8B8));
        assert_eq!(fmt(PF_RGB | PF_ALPHA_PIXELS, 32, [0xFF, 0xFF00, 0xFF0000, 0xFF000000]), Ok(Format::A8B8G8R8));
        assert_eq!(fmt(PF_RGB, 32, [0xFF, 0xFF00, 0xFF0000, 0]), Ok(Format::X8B8G8R8));
        assert_eq!(fmt(PF_RGB, 16, [0xF800, 0x7E0, 0x1F, 0]), Ok(Format::R5G6B5));
        assert_eq!(fmt(PF_RGB | PF_ALPHA_PIXELS, 16, [0x7C00, 0x3E0, 0x1F, 0x8000]), Ok(Format::A1R5G5B5));
        assert_eq!(fmt(PF_RGB, 16, [0x7C00, 0x3E0, 0x1F, 0]), Ok(Format::X1R5G5B5));
        assert_eq!(fmt(PF_LUMINANCE, 8, [0xFF, 0, 0, 0]), Ok(Format::L8));
        assert_eq!(fmt(PF_LUMINANCE | PF_ALPHA_PIXELS, 16, [0xFF, 0, 0, 0xFF00]), Ok(Format::A8L8));
        assert_eq!(to_rgba8(Format::X8R8G8B8, &[1, 2, 3, 0]).unwrap(), [3, 2, 1, 255]);
        assert_eq!(to_rgba8(Format::A8B8G8R8, &[1, 2, 3, 4]).unwrap(), [1, 2, 3, 4]);
        assert_eq!(to_rgba8(Format::X8B8G8R8, &[1, 2, 3, 0]).unwrap(), [1, 2, 3, 255]);
        assert_eq!(to_rgba8(Format::R5G6B5, &0xF800u16.to_le_bytes()).unwrap(), [255, 0, 0, 255]);
        assert_eq!(to_rgba8(Format::A1R5G5B5, &0x03E0u16.to_le_bytes()).unwrap(), [0, 255, 0, 0]);
        assert_eq!(to_rgba8(Format::A1R5G5B5, &0x801Fu16.to_le_bytes()).unwrap(), [0, 0, 255, 255]);
        assert_eq!(to_rgba8(Format::X1R5G5B5, &0x001Fu16.to_le_bytes()).unwrap(), [0, 0, 255, 255]);
        assert_eq!(to_rgba8(Format::L8, &[7]).unwrap(), [7, 7, 7, 255]);
        assert_eq!(to_rgba8(Format::A8L8, &[7, 9]).unwrap(), [7, 7, 7, 9]);
        // A level is w × h × the bytes of one texel.
        assert_eq!(Format::L8.level_size(4, 2), 8);
        assert_eq!(Format::X8B8G8R8.level_size(4, 2), 32);
    }

    #[test]
    fn bad_input() {
        assert!(matches!(DdsHeader::parse(b"XXXX"), Err(DdsError::BadMagic(_))));
        assert_eq!(DdsHeader::parse(b"DDS \0\0"), Err(DdsError::Truncated(6)));
    }

    /// A side of 0 or past the cap is refused; the overflowing header that used to parse
    /// with an empty payload (0x80000000 square A8R8G8B8: its size wrapped to 0) too [#1995].
    #[test]
    fn sizes_out_of_range_are_refused() {
        let argb = |w: u32, h: u32, payload: usize| build(w, h, 1, PF_RGB | PF_ALPHA_PIXELS, b"\0\0\0\0", 32, [0x00FF_0000, 0x0000_FF00, 0x0000_00FF, 0xFF00_0000], 0, payload);
        let huge = argb(0x8000_0000, 0x8000_0000, 0);
        assert_eq!(Dds::parse(&huge).unwrap_err(), DdsError::BadSize { width: 0x8000_0000, height: 0x8000_0000 });
        assert!(matches!(Dds::parse(&argb(0, 4, 16)), Err(DdsError::BadSize { width: 0, height: 4 })));
        assert!(matches!(Dds::parse(&argb(4, 0, 16)), Err(DdsError::BadSize { .. })));
        assert!(matches!(Dds::parse(&argb(MAX_SIDE * 2, 1, 0)), Err(DdsError::BadSize { .. })));
        assert!(Dds::parse(&argb(4, 4, 64)).is_ok());
        // The listing reads the header as it is (fnvtool dds list).
        assert_eq!(DdsHeader::parse(&huge).unwrap().width, 0x8000_0000);
        // Sizes saturate rather than wrap.
        assert_eq!(Format::A8R8G8B8.level_size(u32::MAX, u32::MAX), (u32::MAX as usize).saturating_mul(u32::MAX as usize).saturating_mul(4));
        assert_ne!(Format::A8R8G8B8.level_size(0x8000_0000, 0x8000_0000), 0);
    }

    proptest::proptest! {
        #[test]
        fn never_panics(data in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..300)) {
            let _ = Dds::parse(&data);
        }

        #[test]
        fn header_fuzz_never_panics(w in 0u32..u32::MAX, h in 0u32..u32::MAX, mips in 0u32..u32::MAX) {
            let d = build(w, h, mips, PF_FOURCC, b"DXT1", 0, [0; 4], 0, 64);
            let _ = Dds::parse(&d);
        }
    }
}

#[cfg(test)]
mod bc5_tests {
    use super::*;

    fn dds(fourcc: &[u8; 4], w: u32, h: u32, mips: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_LEN];
        bytes[..4].copy_from_slice(b"DDS ");
        for (offset, value) in [(4, 124), (8, 0x21007), (12, h), (16, w), (28, mips), (76, 32), (80, 4)] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[84..88].copy_from_slice(fourcc);
        bytes.extend_from_slice(payload);
        bytes
    }

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() <= 2.0e-7, "{actual} != {expected}");
    }

    #[test]
    fn legacy_bc5_headers_size_mips_and_dx10_boundary() {
        for (tag, format) in [(b"BC5S", Format::Bc5S), (b"ATI2", Format::Bc5U)] {
            let data = dds(tag, 5, 3, 2, &[0; 48]);
            let parsed = Dds::parse(&data).unwrap();
            assert_eq!(parsed.format, format);
            assert_eq!(parsed.levels[0].iter().map(|p| p.len()).collect::<Vec<_>>(), [32, 16]);
            assert_eq!(parsed.header.level_dims(1), (2, 1));
            let mut trailing = data.clone(); trailing.extend_from_slice(&[99; 7]);
            assert_eq!(Dds::parse(&trailing).unwrap().levels, parsed.levels);
            assert_eq!(Dds::parse(&data[..data.len() - 1]).unwrap_err(), DdsError::SizeMismatch { expected: 48, actual: 47 });
        }
        assert!(matches!(Dds::parse(&dds(b"BC5U", 4, 4, 1, &[0; 16])), Err(DdsError::Unsupported(_))));
        // A DX10 header naming BC5_SNORM with resource dimension and array size left at
        // zero is malformed and stays refused.
        let mut dx10 = dds(b"DX10", 4, 4, 1, &[0; 36]);
        dx10[128..132].copy_from_slice(&84u32.to_le_bytes());
        assert!(matches!(Dds::parse(&dx10), Err(DdsError::Unsupported(_))));
        // The same with a 2D dimension and an array of one is read, its data after the
        // 20-byte extension.
        let mut block = [0u8; 16]; block[0] = 129; block[8] = 127;
        let mut payload = vec![0u8; 20]; payload.extend_from_slice(&block);
        let mut good = dds(b"DX10", 4, 4, 1, &payload);
        for (offset, value) in [(128, 84u32), (132, 3), (140, 1)] {
            good[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        let parsed = Dds::parse(&good).unwrap();
        assert_eq!(parsed.format, Format::Bc5S);
        assert_eq!(parsed.header.data_offset(), 148);
        assert_eq!(parsed.levels[0], [&block[..]]);
        assert_eq!(decode_bc5(parsed.format, 4, 4, parsed.levels[0][0]).unwrap()[0], [-1.0, 1.0]);
    }

    #[test]
    fn signed_palettes_are_independent_and_preserve_fractional_values() {
        // Literal packed indices 0,1,2,3,4,5,6,7 twice, in each channel.
        let block = [127, 129, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa,
                     129, 127, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa];
        let pixels = decode_bc5(Format::Bc5S, 4, 4, &block).unwrap();
        let red = [1.0, -1.0, 5.0/7.0, 3.0/7.0, 1.0/7.0, -1.0/7.0, -3.0/7.0, -5.0/7.0];
        let green = [-1.0, 1.0, -0.6, -0.2, 0.2, 0.6, -1.0, 1.0];
        for i in 0..16 { close(pixels[i][0], red[i % 8]); close(pixels[i][1], green[i % 8]); }
        assert!(to_rgba8(Format::Bc5S, &block).is_none());
        assert!(decode_blocks(Format::Bc5S, 4, 4, &block).is_none());
    }

    #[test]
    fn signed_minimum_alias_is_clamped_but_comparison_stays_signed() {
        // -127 > -128 uses six interpolated values, all -1; index7 must not become +1.
        let block = [129, 128, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
                     128, 127, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa];
        let pixels = decode_bc5(Format::Bc5S, 4, 4, &block).unwrap();
        let green = [-1.0, 1.0, -0.6, -0.2, 0.2, 0.6, -1.0, 1.0];
        for i in 0..16 { assert_eq!(pixels[i][0], -1.0); close(pixels[i][1], green[i % 8]); }
        let zero = decode_bc5(Format::Bc5S, 4, 4, &[0; 16]).unwrap();
        assert!(zero.iter().all(|p| *p == [0.0, 0.0]));
    }

    #[test]
    fn unsigned_neighbor_does_not_reinterpret_endpoint_signs() {
        let block = [255, 0, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa,
                     0, 255, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa];
        let pixels = decode_bc5(Format::Bc5U, 4, 4, &block).unwrap();
        let red = [1.0, 0.0, 6.0/7.0, 5.0/7.0, 4.0/7.0, 3.0/7.0, 2.0/7.0, 1.0/7.0];
        let green = [0.0, 1.0, 0.2, 0.4, 0.6, 0.8, 0.0, 1.0];
        for i in 0..16 { close(pixels[i][0], red[i % 8]); close(pixels[i][1], green[i % 8]); }
        close(decode_bc5(Format::Bc5S, 4, 4, &block).unwrap()[0][0], -1.0/127.0);
    }

    #[test]
    fn partial_edge_blocks_are_cropped_and_bad_sizes_refused() {
        let mut blocks = [0; 32]; blocks[0] = 129; blocks[8] = 127; blocks[16] = 127;
        let pixels = decode_bc5(Format::Bc5S, 3, 5, &blocks).unwrap();
        assert_eq!(pixels.len(), 15);
        assert!(pixels[..12].iter().all(|p| *p == [-1.0, 1.0]));
        assert_eq!(&pixels[12..], &[[1.0, 0.0]; 3]);
        assert!(decode_bc5(Format::Bc5S, 3, 5, &blocks[..31]).is_none());
        for (w, h) in [(0, 4), (4, 0), (MAX_SIDE + 1, 4), (4, u32::MAX)] {
            assert!(decode_bc5(Format::Bc5S, w, h, &blocks).is_none());
        }
        assert!(decode_bc5(Format::Dxt5, 4, 4, &blocks).is_none());
    }
}

#[cfg(test)]
mod dx10_tests {
    use super::*;

    /// The fields of a DX10 extension header, the ones a valid 2D texture has by default.
    #[derive(Clone, Copy)]
    struct Ext {
        dxgi: u32,
        dimension: u32,
        misc: u32,
        array: u32,
        flags2: u32,
    }

    fn ext(dxgi: u32) -> Ext {
        Ext { dxgi, dimension: 3, misc: 0, array: 1, flags2: 0 }
    }

    /// A DX10 file generated here: the 128-byte header with FourCC `DX10`, the 20-byte
    /// extension, then `payload` bytes of 0xAB.
    fn dx10(e: Ext, w: u32, h: u32, mips: u32, caps2: u32, payload: usize) -> Vec<u8> {
        let mut d = vec![0u8; HEADER_LEN + DX10_HEADER_LEN];
        d[..4].copy_from_slice(b"DDS ");
        let put = |d: &mut Vec<u8>, o: usize, v: u32| d[o..o + 4].copy_from_slice(&v.to_le_bytes());
        for (o, v) in [(4, 124), (8, 0x2_1007), (12, h), (16, w), (28, mips), (76, 32), (80, PF_FOURCC), (108, 0x1000), (112, caps2)] {
            put(&mut d, o, v);
        }
        d[84..88].copy_from_slice(b"DX10");
        for (k, v) in [e.dxgi, e.dimension, e.misc, e.array, e.flags2].into_iter().enumerate() {
            put(&mut d, HEADER_LEN + 4 * k, v);
        }
        d.resize(HEADER_LEN + DX10_HEADER_LEN + payload, 0xAB);
        d
    }

    #[test]
    fn every_accepted_dxgi_code_names_its_format_hint_and_offset() {
        let expected = [
            (28, Format::A8B8G8R8, Some(false)), (29, Format::A8B8G8R8, Some(true)),
            (71, Format::Dxt1, Some(false)), (72, Format::Dxt1, Some(true)),
            (74, Format::Dxt3, Some(false)), (75, Format::Dxt3, Some(true)),
            (77, Format::Dxt5, Some(false)), (78, Format::Dxt5, Some(true)),
            (80, Format::Bc4U, Some(false)), (81, Format::Bc4S, Some(false)),
            (83, Format::Bc5U, Some(false)), (84, Format::Bc5S, Some(false)),
            (87, Format::A8R8G8B8, Some(false)), (91, Format::A8R8G8B8, Some(true)),
            (98, Format::Bc7, Some(false)), (99, Format::Bc7, Some(true)),
        ];
        assert_eq!(expected.len(), DXGI_FORMATS.len());
        for (code, format, srgb) in expected {
            let d = dx10(ext(code), 4, 4, 1, 0, format.level_size(4, 4));
            let t = Dds::parse(&d).unwrap_or_else(|e| panic!("dxgi {code}: {e}"));
            assert_eq!(t.format, format, "dxgi {code}");
            assert_eq!(t.srgb_hint(), srgb, "dxgi {code}");
            assert_eq!(t.header.data_offset(), 148);
            assert_eq!(t.levels[0][0].len(), format.level_size(4, 4));
            assert_eq!(t.header.format_name().unwrap(), dxgi_name(code).unwrap());
            assert!(!t.alpha_opaque());
        }
        assert_eq!(dxgi_name(99), Some("BC7_UNORM_SRGB"));
        assert_eq!(dxgi_name(80), Some("BC4_UNORM"));
        // Block sizes: BC4 8 bytes a block, BC7 16.
        assert_eq!((Format::Bc4U.block_bytes(), Format::Bc4S.block_bytes(), Format::Bc7.block_bytes()), (Some(8), Some(8), Some(16)));
        // DX10 R8G8B8A8 and B8G8R8A8 convert as the legacy layouts they are.
        let rgba_bytes = dx10(ext(28), 1, 1, 1, 0, 4);
        let rgba = Dds::parse(&rgba_bytes).unwrap();
        assert_eq!(to_rgba8(rgba.format, &[1, 2, 3, 4]).unwrap(), [1, 2, 3, 4]);
        let bgra_bytes = dx10(ext(87), 1, 1, 1, 0, 4);
        let bgra = Dds::parse(&bgra_bytes).unwrap();
        assert_eq!(to_rgba8(bgra.format, &[1, 2, 3, 4]).unwrap(), [3, 2, 1, 4]);
        // A legacy file has no hint and its data at 128.
        let mut legacy = vec![0u8; HEADER_LEN];
        legacy[..4].copy_from_slice(b"DDS ");
        for (o, v) in [(12usize, 4u32), (16, 4), (28, 1), (80, PF_FOURCC)] {
            legacy[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        legacy[84..88].copy_from_slice(b"BC4U");
        legacy.extend_from_slice(&[0; 8]);
        let t = Dds::parse(&legacy).unwrap();
        assert_eq!((t.format, t.srgb_hint(), t.header.data_offset(), t.alpha_opaque()), (Format::Bc4U, None, 128, false));
        legacy[84..88].copy_from_slice(b"BC4S");
        assert_eq!(Dds::parse(&legacy).unwrap().format, Format::Bc4S);
        // `ATI1` is the older code for BC4 unsigned (#3151): the same reading as `BC4U`.
        legacy[84..88].copy_from_slice(b"ATI1");
        let t = Dds::parse(&legacy).unwrap();
        assert_eq!((t.format, t.srgb_hint(), t.header.data_offset(), t.alpha_opaque()), (Format::Bc4U, None, 128, false));
        assert_eq!(decode_bc4(t.format, 4, 4, t.levels[0][0]).unwrap(), [0.0; 16]);
        // The size guards hold for it too (#1995): a block short, and a side past the cap.
        assert!(Dds::parse(&legacy[..legacy.len() - 1]).is_err());
        legacy[16..20].copy_from_slice(&0x8000_0000u32.to_le_bytes());
        assert!(Dds::parse(&legacy).is_err());
        // Other codes stay refused.
        legacy[16..20].copy_from_slice(&4u32.to_le_bytes());
        legacy[84..88].copy_from_slice(b"ATI3");
        assert!(matches!(Dds::parse(&legacy), Err(DdsError::Unsupported(_))));
    }

    #[test]
    fn a_dx10_file_shorter_than_its_two_headers_is_truncated() {
        let d = dx10(ext(98), 4, 4, 1, 0, 16);
        assert!(Dds::parse(&d).is_ok());
        for len in [128, 140, 147] {
            assert_eq!(DdsHeader::parse(&d[..len]), Err(DdsError::Truncated(len)));
            assert_eq!(Dds::parse(&d[..len]).unwrap_err(), DdsError::Truncated(len));
        }
        // Headers whole, a level short: the payload is counted from 148.
        assert_eq!(Dds::parse(&d[..d.len() - 1]).unwrap_err(), DdsError::SizeMismatch { expected: 16, actual: 15 });
        assert_eq!(Dds::parse(&d[..148]).unwrap_err(), DdsError::SizeMismatch { expected: 16, actual: 0 });
    }

    /// Full chains down to 1×1 lay out with whole blocks per level, as the legacy formats
    /// do; bytes past the counted levels are left alone.
    #[test]
    fn full_mip_chains_lay_out_to_one_texel() {
        // 4096² BC7, 13 levels: 1024² blocks down to one block for 4², 2², 1².
        let sizes: Vec<usize> = (0..13).map(|i| ((4096usize >> i).max(1).div_ceil(4)).pow(2) * 16).collect();
        let total: usize = sizes.iter().sum();
        let t = dx10(ext(99), 4096, 4096, 13, 0, total + 5);
        let p = Dds::parse(&t).unwrap();
        assert_eq!(p.levels[0].iter().map(|l| l.len()).collect::<Vec<_>>(), sizes);
        assert_eq!(&sizes[10..], &[16, 16, 16]);
        assert_eq!(p.header.level_dims(12), (1, 1));
        assert_eq!(Dds::parse(&t[..148 + total - 1]).unwrap_err(), DdsError::SizeMismatch { expected: total, actual: total - 1 });
        // 8×4 BC7: 8×4, 4×2, 2×1, 1×1 = 2 + 1 + 1 + 1 blocks.
        let p8 = dx10(ext(98), 8, 4, 4, 0, 80);
        assert_eq!(Dds::parse(&p8).unwrap().levels[0].iter().map(|l| l.len()).collect::<Vec<_>>(), [32, 16, 16, 16]);
        // 5×3 BC4: 5×3 (2×1 blocks), 2×1, 1×1, 8 bytes a block.
        let p5 = dx10(ext(80), 5, 3, 3, 0, 32);
        let p = Dds::parse(&p5).unwrap();
        assert_eq!(p.levels[0].iter().map(|l| l.len()).collect::<Vec<_>>(), [16, 8, 8]);
        assert_eq!(p.header.level_dims(1), (2, 1));
        let mut long = p5.clone();
        long.extend_from_slice(&[7; 9]);
        assert_eq!(Dds::parse(&long).unwrap().levels, p.levels);
    }

    #[test]
    fn dx10_files_other_than_one_flat_2d_texture_are_refused() {
        let refused = |e: Ext, caps2: u32, payload: usize| match Dds::parse(&dx10(e, 4, 4, 1, caps2, payload)) {
            Err(DdsError::Unsupported(why)) => why,
            other => panic!("{other:?}"),
        };
        for dimension in [0, 1, 2, 4, 5] {
            assert!(refused(Ext { dimension, ..ext(98) }, 0, 16).contains("dimension"));
        }
        for array in [0, 2, 6] {
            assert!(refused(Ext { array, ..ext(98) }, 0, 96).contains("array"));
        }
        // A cube by the DX10 flag, or by the legacy capability bits on a DX10 file.
        assert!(refused(Ext { misc: 0x4, ..ext(98) }, 0, 96).contains("cube"));
        assert!(refused(ext(98), CAPS2_CUBEMAP | CAPS2_ALL_FACES, 96).contains("cube"));
        let cube = DdsHeader::parse(&dx10(Ext { misc: 0x4, ..ext(98) }, 4, 4, 1, 0, 96)).unwrap();
        assert!(cube.is_cubemap());
        assert_eq!(cube.faces(), 6);
        // Codes outside the table: BC6H, BC1_TYPELESS, unknown.
        for code in [0, 70, 79, 95, 97, 100, u32::MAX] {
            assert_eq!(refused(ext(code), 0, 16), format!("unknown(dxgi={code})"));
            assert!(dxgi_name(code).is_none());
        }
        // The other misc bits and a mip count of 0 are not refusals.
        assert!(Dds::parse(&dx10(Ext { misc: 0x1, ..ext(98) }, 4, 4, 0, 0, 16)).is_ok());
    }

    /// What the header-only listing sizes: arrays count every element, a cube six faces
    /// per element, a volume each level's slices; layouts it cannot size say so.
    #[test]
    fn arrays_cube_arrays_and_volumes_are_sized_whole() {
        let header = |e: Ext, mips: u32, depth: u32| {
            let mut d = dx10(e, 4, 4, mips, 0, 0);
            d[24..28].copy_from_slice(&depth.to_le_bytes());
            DdsHeader::parse(&d).unwrap()
        };
        let array = header(Ext { array: 2, ..ext(98) }, 1, 0);
        assert!(array.layout_sized());
        assert_eq!((array.faces(), array.array_elements(), array.expected_size(Format::Bc7)), (1, 2, 32));
        let cubes = header(Ext { misc: 0x4, array: 2, ..ext(98) }, 1, 0);
        assert_eq!((cubes.faces(), cubes.expected_size(Format::Bc7)), (6, 6 * 2 * 16));
        // 4×4×4 BC7, 3 levels: 4 slices, then 2, then 1, one block each.
        let volume = header(Ext { dimension: 4, ..ext(98) }, 3, 4);
        assert!(volume.layout_sized());
        assert_eq!(volume.expected_size(Format::Bc7), (4 + 2 + 1) * 16);
        // 1D arrays are sized like 2D ones.
        assert_eq!(header(Ext { dimension: 2, array: 3, ..ext(98) }, 1, 0).expected_size(Format::Bc7), 48);
        for e in [Ext { array: 0, ..ext(98) }, Ext { dimension: 0, ..ext(98) }, Ext { dimension: 4, array: 2, ..ext(98) }, Ext { dimension: 4, misc: 0x4, ..ext(98) }, Ext { dimension: 2, misc: 0x4, ..ext(98) }] {
            assert!(!header(e, 1, 4).layout_sized());
        }
        // A volume or 1D header with only the legacy cube bit is a cube to the listing
        // too, so it is not sized either; a 2D one is (six legacy faces).
        let legacy_cube = |dimension: u32| DdsHeader::parse(&dx10(Ext { dimension, ..ext(98) }, 4, 4, 1, CAPS2_CUBEMAP | CAPS2_ALL_FACES, 0)).unwrap();
        for dimension in [2, 4] {
            let h = legacy_cube(dimension);
            assert!(h.is_cubemap() && !h.layout_sized(), "dimension {dimension}");
        }
        assert!(legacy_cube(3).layout_sized());
        assert_eq!(legacy_cube(3).expected_size(Format::Bc7), 6 * 16);
        // Volumes by the legacy bit or the DX10 dimension.
        assert!(legacy_cube(4).is_volume() && !legacy_cube(3).is_volume());
        let mut legacy = vec![0u8; HEADER_LEN];
        legacy[..4].copy_from_slice(b"DDS ");
        for (o, v) in [(12usize, 4u32), (16, 4), (24, 4), (28, 1), (80, PF_FOURCC), (112, CAPS2_VOLUME)] {
            legacy[o..o + 4].copy_from_slice(&v.to_le_bytes());
        }
        legacy[84..88].copy_from_slice(b"DXT1");
        let legacy_volume = DdsHeader::parse(&legacy).unwrap();
        assert!(legacy_volume.is_volume() && !legacy_volume.layout_sized());
        legacy[112..116].copy_from_slice(&0u32.to_le_bytes());
        assert!(!DdsHeader::parse(&legacy).unwrap().is_volume());
        assert!(DdsHeader::parse(&legacy).unwrap().layout_sized());
        // Legacy files and a flat DX10 2D texture are sized as before.
        assert!(header(ext(98), 1, 0).layout_sized());
        assert_eq!(header(ext(98), 1, 9).expected_size(Format::Bc7), 16, "depth counts for volumes only");
    }

    #[test]
    fn decode_blocks_refuses_sides_out_of_range_before_allocating() {
        let block = [0u8; 16];
        for (w, h) in [(0, 4), (4, 0), (MAX_SIDE + 1, 4), (4, u32::MAX), (u32::MAX, u32::MAX)] {
            assert!(decode_blocks(Format::Dxt5, w, h, &block).is_none(), "{w}x{h}");
        }
        assert_eq!(decode_blocks(Format::Dxt5, 3, 2, &block).unwrap().len(), 3 * 2 * 4);
        assert!(decode_blocks(Format::Dxt5, 5, 4, &block).is_none(), "two blocks needed");
    }

    #[test]
    fn decode_blocks_rect_matches_full_decode_and_handles_4k_subrects() {
        // Non-multiple-of-4 size (10 x 6 = 3 x 2 blocks) across DXT1, DXT3 and DXT5.
        for format in [Format::Dxt1, Format::Dxt3, Format::Dxt5] {
            let block_len = format.block_bytes().unwrap();
            let src: Vec<u8> = (0..6 * block_len).map(|i| ((i * 73 + 19) & 0xFF) as u8).collect();
            let full = decode_blocks(format, 10, 6, &src).unwrap();
            for rect in [[0, 0, 10, 6], [1, 1, 5, 4], [3, 2, 7, 4], [8, 4, 2, 2], [4, 0, 4, 4]] {
                let sub = decode_blocks_rect(format, 10, 6, &src, rect).unwrap();
                let [rx, ry, rw, rh] = rect.map(|v| v as usize);
                let mut expected = Vec::with_capacity(rw * rh * 4);
                for y in ry..ry + rh {
                    expected.extend_from_slice(&full[(y * 10 + rx) * 4..(y * 10 + rx + rw) * 4]);
                }
                assert_eq!(sub, expected, "{format:?} rect={rect:?}");
            }
            assert!(decode_blocks_rect(format, 10, 6, &src, [0, 0, 0, 2]).is_none());
            assert!(decode_blocks_rect(format, 10, 6, &src, [0, 0, 2, 0]).is_none());
            assert!(decode_blocks_rect(format, 10, 6, &src, [9, 0, 2, 2]).is_none());
            assert!(decode_blocks_rect(format, 10, 6, &src, [0, 5, 2, 2]).is_none());
            assert!(decode_blocks_rect(format, 10, 6, &src, [u32::MAX, 0, 2, 2]).is_none());
        }
        // 4096 x 4096 DXT5: decode a non-block-aligned sub-rect without allocating 64 MiB.
        let bw = 1024usize;
        let mut big = vec![0u8; bw * bw * 16];
        let off = (833 * bw + 353) * 16;
        big[off] = 255;
        big[off + 1] = 255;
        big[off + 8..off + 10].copy_from_slice(&0xFFFFu16.to_le_bytes());
        let sub = decode_blocks_rect(Format::Dxt5, 4096, 4096, &big, [1410, 3330, 6, 6]).unwrap();
        assert_eq!(sub.len(), 6 * 6 * 4);
        assert_eq!(&sub[0..4], &[0, 0, 0, 0]);
        assert_eq!(&sub[(2 * 6 + 2) * 4..(2 * 6 + 2) * 4 + 4], &[255, 255, 255, 255]);
        assert!(decode_blocks_rect(Format::Bc7, 4, 4, &[0; 16], [0, 0, 4, 4]).is_none());
    }

    #[test]
    fn to_rgba8_rect_matches_full_decode_and_checks_bounds() {
        for format in [Format::A8R8G8B8, Format::R8G8B8, Format::A4R4G4B4, Format::L8] {
            let bpp = format.bytes_per_pixel().unwrap();
            let src: Vec<u8> = (0..6 * 5 * bpp).map(|i| ((i * 41 + 7) & 0xFF) as u8).collect();
            let full = to_rgba8(format, &src).unwrap();
            for rect in [[0, 0, 6, 5], [1, 2, 4, 2], [5, 4, 1, 1]] {
                let sub = to_rgba8_rect(format, 6, 5, &src, rect).unwrap();
                let [rx, ry, rw, rh] = rect.map(|v| v as usize);
                let mut expected = Vec::with_capacity(rw * rh * 4);
                for y in ry..ry + rh {
                    expected.extend_from_slice(&full[(y * 6 + rx) * 4..(y * 6 + rx + rw) * 4]);
                }
                assert_eq!(sub, expected, "{format:?} rect={rect:?}");
            }
            assert!(to_rgba8_rect(format, 6, 5, &src, [0, 0, 0, 1]).is_none());
            assert!(to_rgba8_rect(format, 6, 5, &src, [5, 0, 2, 1]).is_none());
            assert!(to_rgba8_rect(format, 6, 5, &src, [0, 4, 1, 2]).is_none());
            assert!(to_rgba8_rect(format, 6, 5, &src, [u32::MAX, 0, 1, 1]).is_none());
            assert!(to_rgba8_rect(format, 6, 5, &src[..src.len() - 1], [0, 0, 1, 1]).is_none());
        }
        assert!(to_rgba8_rect(Format::Dxt5, 4, 4, &[0; 16], [0, 0, 4, 4]).is_none());
    }

    #[test]
    fn the_opaque_alpha_mode_is_read_from_the_low_three_bits() {
        for (flags2, opaque) in [(0, false), (1, false), (2, false), (3, true), (4, false), (3 | 0x8, true), (0xB, true), (0x7, false)] {
            let bytes = dx10(Ext { flags2, ..ext(98) }, 4, 4, 1, 0, 16);
            let t = Dds::parse(&bytes).unwrap();
            assert_eq!(t.alpha_opaque(), opaque, "miscFlags2 {flags2:#x}");
            assert_eq!(t.header.dx10.unwrap().alpha_mode(), flags2 & 7);
        }
    }

    #[test]
    fn bc4_palettes_decode_as_one_bc5_channel() {
        // Literal indices 0..7 twice; unsigned 255/0 then 0/255 endpoints.
        let unsigned = [255, 0, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa];
        let v = decode_bc4(Format::Bc4U, 4, 4, &unsigned).unwrap();
        let six = [1.0, 0.0, 6.0 / 7.0, 5.0 / 7.0, 4.0 / 7.0, 3.0 / 7.0, 2.0 / 7.0, 1.0 / 7.0];
        for i in 0..16 { assert!((v[i] - six[i % 8]).abs() <= 2.0e-7, "{i}: {}", v[i]); }
        let four = decode_bc4(Format::Bc4U, 4, 4, &[0, 255, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa]).unwrap();
        let expected = [0.0, 1.0, 0.2, 0.4, 0.6, 0.8, 0.0, 1.0];
        for i in 0..16 { assert!((four[i] - expected[i % 8]).abs() <= 2.0e-7, "{i}: {}", four[i]); }
        // Signed: 127 / -127 with six steps, the extra values -1 / +1 with four.
        let signed = decode_bc4(Format::Bc4S, 4, 4, &[127, 129, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa]).unwrap();
        let s = [1.0, -1.0, 5.0 / 7.0, 3.0 / 7.0, 1.0 / 7.0, -1.0 / 7.0, -3.0 / 7.0, -5.0 / 7.0];
        for i in 0..16 { assert!((signed[i] - s[i % 8]).abs() <= 2.0e-7, "{i}: {}", signed[i]); }
        let signed4 = decode_bc4(Format::Bc4S, 4, 4, &[129, 127, 0x88, 0xc6, 0xfa, 0x88, 0xc6, 0xfa]).unwrap();
        let s4 = [-1.0, 1.0, -0.6, -0.2, 0.2, 0.6, -1.0, 1.0];
        for i in 0..16 { assert!((signed4[i] - s4[i % 8]).abs() <= 2.0e-7, "{i}: {}", signed4[i]); }
        // Partial edge blocks are cropped: 5×3 is two blocks, the second's column 0 kept.
        let mut two = [0u8; 16]; two[0] = 255; two[8] = 0; two[9] = 0;
        let px = decode_bc4(Format::Bc4U, 5, 3, &two).unwrap();
        assert_eq!(px.len(), 15);
        assert!(px.iter().enumerate().all(|(i, &p)| p == if i % 5 < 4 { 1.0 } else { 0.0 }));
        // Short data, bad sizes and other formats are refused; other decoders refuse BC4/BC7.
        assert!(decode_bc4(Format::Bc4U, 5, 3, &two[..15]).is_none());
        for (w, h) in [(0, 4), (4, 0), (MAX_SIDE + 1, 4)] {
            assert!(decode_bc4(Format::Bc4U, w, h, &two).is_none());
        }
        for f in [Format::Bc5U, Format::Bc7, Format::Dxt1] {
            assert!(decode_bc4(f, 4, 4, &two).is_none());
        }
        for f in [Format::Bc4U, Format::Bc4S, Format::Bc7] {
            assert!(to_rgba8(f, &two).is_none());
            assert!(decode_blocks(f, 4, 4, &two).is_none());
            assert!(decode_bc5(f, 4, 4, &two).is_none());
        }
    }

    proptest::proptest! {
        #[test]
        fn dx10_header_fuzz_never_panics(
            dxgi in proptest::prop_oneof![0u32..128, proptest::prelude::any::<u32>()],
            dimension in 0u32..6, misc in proptest::prelude::any::<u32>(), array in 0u32..4,
            flags2 in proptest::prelude::any::<u32>(), w in 0u32..70000, h in 0u32..70000,
            mips in 0u32..40, caps2 in proptest::prelude::any::<u32>(), payload in 0usize..600, cut in 0usize..800,
        ) {
            let d = dx10(Ext { dxgi, dimension, misc, array, flags2 }, w, h, mips, caps2, payload);
            let cut = cut.min(d.len());
            for bytes in [&d[..], &d[..cut]] {
                if let Ok(t) = Dds::parse(bytes) {
                    let _ = (t.srgb_hint(), t.alpha_opaque(), t.header.faces());
                    for (m, level) in t.levels[0].iter().enumerate() {
                        let (lw, lh) = t.header.level_dims(m as u32);
                        let _ = decode_bc4(t.format, lw, lh, level);
                        let _ = decode_bc5(t.format, lw, lh, level);
                        let _ = decode_blocks(t.format, lw, lh, level);
                        let _ = to_rgba8(t.format, level);
                    }
                }
                if let Ok(h) = DdsHeader::parse(bytes) {
                    let _ = (h.format(), h.format_name(), h.expected_size(h.format().unwrap_or(Format::Bc7)), h.data_offset());
                }
            }
        }

        #[test]
        fn random_bytes_after_a_dx10_fourcc_never_panic(tail in proptest::collection::vec(proptest::prelude::any::<u8>(), 0..200)) {
            let mut d = dx10(ext(98), 4, 4, 1, 0, 0);
            d.truncate(HEADER_LEN);
            d.extend_from_slice(&tail);
            let _ = Dds::parse(&d);
        }
    }
}
