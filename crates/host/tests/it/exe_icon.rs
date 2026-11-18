//! floti.exe carries the desktop app's icon, `packages/desktop/build/icon.ico`,
//! as its icon resource, so Explorer, the taskbar and Task Manager show the
//! colour circle for it. `build.rs` compiles it in from `floti.rc`.
//!
//! The test reads the built `floti`'s own PE resources: its one icon group
//! lists every image of icon.ico, size for size and in order, and each icon
//! the group names holds exactly that image's bytes. A `floti` built for
//! macOS or Linux is no PE and carries no Windows resources, so there the
//! test ends at once: there is nothing to look for.

use std::path::Path;

use object::FileKind;
use object::LittleEndian as LE;
use object::pe::ImageResourceDirectoryEntry;
use object::pe::{RT_GROUP_ICON, RT_ICON};
use object::read::pe::{PeFile64, ResourceDirectoryEntryData};

/// One image an icon lists: its size, and the bytes of the image itself.
#[derive(PartialEq, Eq)]
struct Image<'a> {
    width: u8,
    height: u8,
    bytes: &'a [u8],
}

/// A little-endian `u16` at `at`.
fn u16_at(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([data[at], data[at + 1]])
}

/// A little-endian `u32` at `at`.
fn u32_at(data: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([data[at], data[at + 1], data[at + 2], data[at + 3]])
}

/// The header both an .ico file and an icon group begin with: reserved,
/// type 1 (an icon), then the number of images.
fn image_count(data: &[u8], what: &str) -> usize {
    assert!(data.len() >= 6, "{what} is shorter than its header");
    assert_eq!(u16_at(data, 2), 1, "{what} is not an icon");
    usize::from(u16_at(data, 4))
}

/// The images of an .ico file, in the order its directory lists them. Each
/// directory entry is 16 bytes: width, height, colours, reserved, planes,
/// bits per pixel, the image's length and its offset in the file.
fn ico_images(ico: &[u8]) -> Vec<Image<'_>> {
    (0..image_count(ico, "icon.ico"))
        .map(|index| {
            let entry = 6 + index * 16;
            let length = u32_at(ico, entry + 8) as usize;
            let offset = u32_at(ico, entry + 12) as usize;
            Image {
                width: ico[entry],
                height: ico[entry + 1],
                bytes: &ico[offset..offset + length],
            }
        })
        .collect()
}

/// Every resource of `kind` in the image, as its id and its bytes in the
/// first language it has.
fn resources_of<'d>(binary: &'d [u8], pe: &PeFile64<'d>, kind: u16) -> Vec<(u16, &'d [u8])> {
    let sections = pe.section_table();
    let Some(directory) = pe
        .data_directories()
        .resource_directory(binary, &sections)
        .expect("read floti's resource directory")
    else {
        return Vec::new();
    };
    let table = |entry: &ImageResourceDirectoryEntry| match entry
        .data(directory)
        .expect("read a resource table")
    {
        ResourceDirectoryEntryData::Table(table) => table,
        ResourceDirectoryEntryData::Data(_) => {
            panic!("a resource entry is data where a table belongs")
        }
    };
    let root = directory.root().expect("read floti's resource root");
    let Some(of_kind) = root
        .entries
        .iter()
        .find(|entry| entry.name_or_id().id() == Some(kind))
    else {
        return Vec::new();
    };
    table(of_kind)
        .entries
        .iter()
        .map(|named| {
            let id = named
                .name_or_id()
                .id()
                .expect("floti.rc numbers its resources");
            let language = table(named)
                .entries
                .first()
                .expect("a resource has a language")
                .data(directory)
                .expect("read a resource's data entry");
            let ResourceDirectoryEntryData::Data(entry) = language else {
                panic!("resource {id} is a table where its data belongs");
            };
            let at = sections
                .pe_data_at(binary, entry.offset_to_data.get(LE))
                .expect("a resource's data lies in a section");
            (id, &at[..entry.size.get(LE) as usize])
        })
        .collect()
}

/// The images floti's one icon group lists, each the bytes of the icon
/// resource it names. A group entry is 14 bytes: width, height, colours,
/// reserved, planes, bits per pixel, the image's length and the icon's id.
fn group_images<'d>(binary: &'d [u8], pe: &PeFile64<'d>) -> Vec<Image<'d>> {
    let groups = resources_of(binary, pe, RT_GROUP_ICON);
    assert_eq!(
        groups.len(),
        1,
        "floti.exe should carry one icon group, the app's; it carries {}",
        groups.len()
    );
    let icons = resources_of(binary, pe, RT_ICON);
    let group = groups[0].1;
    (0..image_count(group, "floti.exe's icon group"))
        .map(|index| {
            let entry = 6 + index * 14;
            let id = u16_at(group, entry + 12);
            let bytes = icons
                .iter()
                .find(|(icon, _)| *icon == id)
                .unwrap_or_else(|| panic!("the icon group names icon {id}, which floti.exe lacks"))
                .1;
            assert_eq!(
                bytes.len(),
                u32_at(group, entry + 8) as usize,
                "icon {id} is not the length its group gives"
            );
            Image {
                width: group[entry],
                height: group[entry + 1],
                bytes,
            }
        })
        .collect()
}

#[test]
fn floti_exe_carries_the_app_icon() {
    let binary = std::fs::read(env!("CARGO_BIN_EXE_floti")).expect("read the built floti");
    // `object` is built with the PE reader only, so for an ELF or Mach-O floti
    // `parse` answers an error rather than a kind. Either way it is no PE:
    // skip. On Windows the binary must be a PE64, or the check proves nothing.
    match FileKind::parse(&*binary) {
        Ok(FileKind::Pe64) => {}
        other => {
            assert!(
                !cfg!(windows),
                "the built floti.exe should be a 64-bit PE; it is {other:?}"
            );
            return;
        }
    }
    let icon_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("packages")
        .join("desktop")
        .join("build")
        .join("icon.ico");
    let ico = std::fs::read(&icon_path).expect("read the app's icon.ico");
    let pe = PeFile64::parse(&*binary).expect("parse floti.exe");

    let carried = group_images(&binary, &pe);
    let app = ico_images(&ico);
    let sizes = |images: &[Image<'_>]| {
        images
            .iter()
            .map(|image| (image.width, image.height))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        sizes(&carried),
        sizes(&app),
        "floti.exe's icon should hold icon.ico's sizes, in its order"
    );
    assert!(
        carried == app,
        "floti.exe's icon images should be icon.ico's, byte for byte"
    );
}
