//! Build-time configuration of the `workshop` executable.
//!
//! On Windows it also writes the executable's Win32 resources, the icon
//! Explorer shows and the version information of the file properties, as a
//! `.res` file. The MSVC linker reads `.res` input itself, so the build needs no
//! resource compiler, Windows SDK lookup or `.rc` text. Formats:
//! [resource file](https://learn.microsoft.com/en-us/windows/win32/menurc/resourceheader),
//! [icon group](https://learn.microsoft.com/en-us/windows/win32/menurc/newheader),
//! [version information](https://learn.microsoft.com/en-us/windows/win32/menurc/vs-versioninfo).

use std::{env, error::Error, fs, path::PathBuf};

type BuildResult<T> = Result<T, Box<dyn Error>>;

const ICON: &str = "assets/icon.ico";
const PRODUCT_NAME: &str = "Holuto Workshop";
const FILE_DESCRIPTION: &str = "Holuto Workshop · drukarki fiskalne";
/// Polish (Poland), the language of the strings above.
const LANGUAGE: u16 = 0x0415;
/// UTF-16, the encoding of every version string.
const CODE_PAGE: u16 = 1200;

const RT_ICON: u16 = 3;
const RT_GROUP_ICON: u16 = 14;
const RT_VERSION: u16 = 16;

fn main() -> BuildResult<()> {
    println!("cargo::rerun-if-changed={ICON}");
    println!("cargo::rerun-if-env-changed=SENTRY_DSN");
    println!("cargo::rerun-if-env-changed=SENTRY_ENVIRONMENT");
    if env::var("PROFILE").as_deref() == Ok("debug") {
        println!("cargo::rustc-env=SLINT_EMIT_DEBUG_INFO=1");
    }
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        if env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
            return Err(
                "Windows builds need an MSVC target: GNU ld cannot link a .res file".into(),
            );
        }
        let out_dir = env::var_os("OUT_DIR").ok_or("Cargo did not set OUT_DIR")?;
        let path = PathBuf::from(out_dir).join("workshop.res");
        fs::write(&path, windows_resources()?)?;
        println!("cargo::rustc-link-arg-bin=workshop={}", path.display());
    }
    Ok(())
}

fn windows_resources() -> BuildResult<Vec<u8>> {
    let ico = fs::read(ICON)?;
    let (group, images) = icon_resources(&ico)?;
    let mut res = Vec::new();
    // The empty first entry marks a 32-bit resource file.
    push_resource(&mut res, 0, 0, 0, &[])?;
    for (id, image) in images {
        push_resource(&mut res, RT_ICON, id, LANGUAGE, image)?;
    }
    // Explorer shows the icon group with the lowest name.
    push_resource(&mut res, RT_GROUP_ICON, 1, LANGUAGE, &group)?;
    push_resource(&mut res, RT_VERSION, 1, LANGUAGE, &version_info()?)?;
    Ok(res)
}

/// Appends a `RESOURCEHEADER` with a numeric type and name, the data and its
/// DWORD padding. The memory flags stay 0: Win32 ignores them.
fn push_resource(
    res: &mut Vec<u8>,
    kind: u16,
    name: u16,
    language: u16,
    data: &[u8],
) -> BuildResult<()> {
    const HEADER_SIZE: u32 = 32;
    const ORDINAL: u16 = 0xFFFF;
    res.extend(u32::try_from(data.len())?.to_le_bytes());
    res.extend(HEADER_SIZE.to_le_bytes());
    res.extend(
        [ORDINAL, kind, ORDINAL, name]
            .into_iter()
            .flat_map(u16::to_le_bytes),
    );
    // DataVersion and MemoryFlags
    res.extend([0; 6]);
    res.extend(language.to_le_bytes());
    // Version and Characteristics
    res.extend([0; 8]);
    res.extend_from_slice(data);
    pad(res);
    Ok(())
}

/// The `RT_GROUP_ICON` directory and the images it names.
type IconResources<'a> = (Vec<u8>, Vec<(u16, &'a [u8])>);

/// Splits an `.ico` file into its images, named 1, 2, …, and the
/// `RT_GROUP_ICON` directory listing them under those names.
fn icon_resources(ico: &[u8]) -> BuildResult<IconResources<'_>> {
    const TRUNCATED: &str = "the icon file is truncated";
    let &[r0, r1, t0, t1, n0, n1] = ico.first_chunk().ok_or(TRUNCATED)?;
    let count = u16::from_le_bytes([n0, n1]);
    if u16::from_le_bytes([r0, r1]) != 0 || u16::from_le_bytes([t0, t1]) != 1 || count == 0 {
        return Err(format!("{ICON} is not an icon file").into());
    }
    let mut group = Vec::new();
    group.extend([0, 1, count].into_iter().flat_map(u16::to_le_bytes));
    let mut images = Vec::new();
    for id in 1..=count {
        let start = 6 + 16 * usize::from(id - 1);
        let entry: [u8; 16] = ico.get(start..start + 16).ok_or(TRUNCATED)?.try_into()?;
        let [fields @ .., s0, s1, s2, s3, o0, o1, o2, o3] = entry;
        let size = u32::from_le_bytes([s0, s1, s2, s3]);
        let offset = usize::try_from(u32::from_le_bytes([o0, o1, o2, o3]))?;
        let end = offset
            .checked_add(usize::try_from(size)?)
            .ok_or(TRUNCATED)?;
        images.push((id, ico.get(offset..end).ok_or(TRUNCATED)?));
        // The file's directory entry with the resource name in place of the file offset.
        group.extend(fields);
        group.extend(size.to_le_bytes());
        group.extend(id.to_le_bytes());
    }
    Ok((group, images))
}

fn version_info() -> BuildResult<Vec<u8>> {
    let version = env::var("CARGO_PKG_VERSION")?;
    let strings = [
        ("ProductName", PRODUCT_NAME),
        ("FileDescription", FILE_DESCRIPTION),
        ("FileVersion", &version),
        ("ProductVersion", &version),
    ]
    .into_iter()
    .map(|(key, text)| version_block(key, Value::Text(text), &[]))
    .collect::<BuildResult<Vec<_>>>()?;
    let table = version_block(
        &format!("{LANGUAGE:04X}{CODE_PAGE:04X}"),
        Value::None,
        &strings,
    )?;
    let translation: Vec<u8> = [LANGUAGE, CODE_PAGE]
        .into_iter()
        .flat_map(u16::to_le_bytes)
        .collect();
    version_block(
        "VS_VERSION_INFO",
        Value::Binary(&fixed_file_info()?),
        &[
            version_block("StringFileInfo", Value::None, &[table])?,
            version_block(
                "VarFileInfo",
                Value::None,
                &[version_block(
                    "Translation",
                    Value::Binary(&translation),
                    &[],
                )?],
            )?,
        ],
    )
}

/// `VS_FIXEDFILEINFO` for an application with version `major.minor.patch.0`.
fn fixed_file_info() -> BuildResult<Vec<u8>> {
    let part = |name: &str| -> BuildResult<u32> { Ok(env::var(name)?.parse::<u16>()?.into()) };
    let high = (part("CARGO_PKG_VERSION_MAJOR")? << 16) | part("CARGO_PKG_VERSION_MINOR")?;
    let low = part("CARGO_PKG_VERSION_PATCH")? << 16;
    let fields: [u32; 13] = [
        // Signature and structure version 1.0
        0xFEEF_04BD,
        0x0001_0000,
        // File and product version
        high,
        low,
        high,
        low,
        // VS_FFI_FILEFLAGSMASK, no flags
        0x3F,
        0,
        // VOS_NT_WINDOWS32, VFT_APP, no subtype, no date
        0x0004_0004,
        1,
        0,
        0,
        0,
    ];
    Ok(fields.into_iter().flat_map(u32::to_le_bytes).collect())
}

#[derive(Clone, Copy)]
enum Value<'a> {
    None,
    Binary(&'a [u8]),
    Text(&'a str),
}

/// One `VS_VERSIONINFO`, `StringFileInfo`, `StringTable`, `String`,
/// `VarFileInfo` or `Var` structure. Offsets are relative to the block, so
/// each child starts DWORD-aligned within its parent.
fn version_block(key: &str, value: Value<'_>, children: &[Vec<u8>]) -> BuildResult<Vec<u8>> {
    // wValueLength counts bytes for binary and UTF-16 units for text; wType 0 is binary.
    let (value, value_length, kind) = match value {
        Value::None => (Vec::new(), 0, 1_u16),
        Value::Binary(bytes) => (bytes.to_vec(), bytes.len(), 0),
        Value::Text(text) => {
            let text = utf16_nul(text);
            let units = text.len() / 2;
            (text, units, 1)
        },
    };
    // wLength, filled in once the block is complete
    let mut block = vec![0; 2];
    block.extend(u16::try_from(value_length)?.to_le_bytes());
    block.extend(kind.to_le_bytes());
    block.extend(utf16_nul(key));
    pad(&mut block);
    block.extend(value);
    for child in children {
        pad(&mut block);
        block.extend_from_slice(child);
    }
    let length = u16::try_from(block.len())?;
    drop(block.splice(..2, length.to_le_bytes()));
    Ok(block)
}

fn utf16_nul(text: &str) -> Vec<u8> {
    text.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

fn pad(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(4), 0);
}
