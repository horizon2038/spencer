#![no_std]
#![no_main]

nun::entry!(main);

fn main(init_info: &nun::InitInfo) {
    nun::println!("Hello, world!");

    nun::println!(
        "version: {}.{}.{}-{}+{}",
        init_info.kernel_major_version,
        init_info.kernel_minor_version,
        init_info.kernel_patch_version,
        init_info.get_pre_release_string(),
        init_info.get_build_metadata_string(),
    );

    loop {}
}
