use std::thread;
use std::time::Duration;
use env_logger::Target;
use log::{debug, info, LevelFilter};
use ril::{Font, Image, TextSegment};
use ril::OverlayMode::Replace;
use crate::pi::e_paper_ws_1in02::{BLACK_PIXEL, HEIGHT, Paper, WHITE_PIXEL, WIDTH};
use crate::pi::font_load;

mod pi;

fn main() {

    env_logger::builder()
        .filter_level(LevelFilter::Debug)
        .target(Target::Stdout)
        .init();

    debug!("main start");

    let mut paper = Paper::new();
    paper.on();
    paper.clear_screen();
    let font = Font::from_bytes(font_load::FONT, 12_f32).unwrap();
    /* 这里高为宽 宽为高 参考原微雪示例程序 方便后续转换 */
    let mut img = Image::new(WIDTH, HEIGHT, WHITE_PIXEL);
    let text_draw = TextSegment::new(&font, "test\ntest,test", BLACK_PIXEL)
        .with_overlay_mode(Replace)
        .with_position(5, 5);
    img.draw(&text_draw);
    paper.display(img);
    thread::sleep(Duration::from_secs(2));
    paper.clear_screen();

    debug!("main end")
}
