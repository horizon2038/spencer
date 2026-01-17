#![no_std]
#![no_main]

nun::entry!(main);

fn main(init_info: &nun::InitInfo) {
    nun::println!("Hello, world!");

    loop {}
}
