use std::ops::Add;
use std::time::{Duration, SystemTime};

use log::{debug, error, info, log_enabled, warn};
use log::Level::Debug;
use ril::{Font, Image, L, TextSegment};
use ril::OverlayMode::Replace;
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};

use crate::pi::{font_load, pi_config};
use crate::pi::buf_type_impl::Bytes;

/// 高（因为是竖向一字节八个像素
pub const HEIGHT: u32 = 80;
/// 宽（因为是竖向一字节八个像素
pub const WIDTH: u32 = 128;
/// 轮询忙状态的间隔
const DEF_AWAIT_BUSY_MS: u64 = 50;
/// 查询忙状态的命令
const CMD_BZ_QUERY: &'static [u8] = &[0x71_u8];
/// 显示命令
const CMD_TO_DISPLAY: &'static [u8] = &[0x12_u8];
/// 设置lut w命令
const CMD_LUT_W_REG: &'static [u8] = &[0x23_u8];
/// 设置lut b命令
const CMD_LUT_B_REG: &'static [u8] = &[0x24_u8];
/// lut 显像电泳数据 初始化时
const PART_LUT_W_REG_DATA: &'static [u8] = &[
    0x60, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x80, 0x1f, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// lut 显像电泳数据 初始化时
const PART_LUT_B_REG_DATA: &'static [u8] = &[
    0x90, 0x01, 0x01, 0x00, 0x00, 0x01,
    0x40, 0x1f, 0x00, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// lut 显像电泳数据 初始化时
const FULL_LUT_W_REG_DATA: &'static [u8] = &[
    0x60, 0x5A, 0x5A, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// lut 显像电泳数据 初始化时
const FULL_LUT_B_REG_DATA: &'static [u8] = &[
    0x90, 0x5A, 0x5A, 0x00, 0x00, 0x01,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
];
/// 灰度 黑色像素值
pub const BLACK_PIXEL: L = L::new(0_u8);
/// 灰度 白色像素值
pub const WHITE_PIXEL: L = L::new(255_u8);
/// spi 发送的全黑的缓冲
const BUF_BLACK_ALL: &'static [u8] = &[0xff_u8; 1280];
/// spi 发送的全白的缓冲
const BUF_WHITE_ALL: &'static [u8] = &[0x00_u8; 1280];


/// 使当前线程sleep等待一定毫秒数，因为是sleep，所以不一定准确，该偏差属可接受范围内
pub fn await_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms))
}

/// 将灰度图ril::Image转为spi发送的数据
/// WIDTH 墨水屏宽
/// HEIGHT 墨水屏高
/// Image 图片数据
pub fn img_2_display_buf(img: &Image<L>) -> Vec<u8> {
    let mut buf = vec![0xff_u8; (HEIGHT * WIDTH / 8) as usize];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let new_x = y;
            let new_y = WIDTH - x - 1;
            if *(img.pixel(x, y)) == BLACK_PIXEL {
                buf[((new_y * HEIGHT + new_x) / 8) as usize] &= !(0x80 >> (y % 8))
            }
        }
    }
    buf
}

pub struct Paper {
    _gpio: Gpio,
    rs_pin: OutputPin,
    dc_pin: OutputPin,
    bz_pin: InputPin,
    pw_pin: OutputPin,
    _spi: Spi,
}

impl Paper {
    pub fn new() -> Self {
        let gpio = Gpio::new().unwrap();
        Paper {
            rs_pin: gpio.get(pi_config::RST_PIN).unwrap().into_output(),
            dc_pin: gpio.get(pi_config::DC_PIN).unwrap().into_output(),
            bz_pin: gpio.get(pi_config::BUSY_PIN).unwrap().into_input(),
            pw_pin: gpio.get(pi_config::PWR_PIN).unwrap().into_output(),
            _gpio: gpio,
            _spi: Spi::new(Bus::Spi0, SlaveSelect::Ss0, 4000000_u32, Mode::Mode0).unwrap(),
        }
    }


    /// 重置
    fn reset(&mut self) {
        self.rs_pin.set_high();
        await_ms(200);
        self.rs_pin.set_low();
        await_ms(2);
        self.rs_pin.set_high();
        await_ms(200);
    }

    fn spi_send(&mut self, buf: &[u8]) {
        match self._spi.write(buf) {
            Ok(_s_size) => {}
            Err(e) => {
                error!("spi send buf fail, err: {e:?}");
                panic!("spi send panic!")
            }
        }
    }

    /// 发送命令
    fn send_cmd<T: Bytes>(&mut self, cmd: T) {
        self.dc_pin.set_low();
        self.spi_send(cmd.bytes());
    }

    /// 发送数据
    fn send_data<T: Bytes>(&mut self, data: T) {
        self.dc_pin.set_high();
        self.spi_send(data.bytes());
    }


    ///当前线程阻塞，等待busy结束，或超时
    fn await_busy(&mut self, timeout_opt: Option<Duration>) -> Result<(), String> {
        if self.on_busy() {
            match timeout_opt {
                Some(dur) => {
                    let dead_time = SystemTime::now().add(dur);
                    while self.on_busy() {
                        if log_enabled!(Debug) {
                            debug!("busy timed await")
                        }
                        if SystemTime::now() > dead_time {
                            warn!("spi await busy timeout");
                            return Err(String::from("spi await busy timeout!"));
                        }
                        await_ms(DEF_AWAIT_BUSY_MS);
                    }
                }
                None => {
                    debug!("loop... busy timed await");
                    loop {
                        if self.on_busy() {
                            await_ms(DEF_AWAIT_BUSY_MS);
                        } else { break; }
                    }
                    debug!("break loop... busy timed await");
                }
            }
        }
        Ok(())
    }

    /// 查询墨水屏是否处于忙状态
    fn on_busy(&mut self) -> bool {
        self.send_cmd(CMD_BZ_QUERY);
        match self.bz_pin.read() {
            Level::High => { false }
            Level::Low => { true }
        }
    }

    /// 使其显示数据
    fn turn_on_display(&mut self) {
        self.send_cmd(CMD_TO_DISPLAY);
        await_ms(10);
        self.await_busy(None).unwrap();
    }


    /// 向其发送lut涌动数据 全屏刷新的
    fn set_full_reg(&mut self) {
        self.send_cmd(CMD_LUT_W_REG);
        self.send_data(FULL_LUT_W_REG_DATA);
        self.send_cmd(CMD_LUT_B_REG);
        self.send_data(FULL_LUT_B_REG_DATA);
    }
    /// 向其发送lut涌动数据 局部刷新的
    fn set_part_reg(&mut self) {
        self.send_cmd(CMD_LUT_W_REG);
        self.send_data(PART_LUT_W_REG_DATA);
        self.send_cmd(CMD_LUT_B_REG);
        self.send_data(PART_LUT_B_REG_DATA);
    }

    /// 初始化墨水品
    pub fn on(&mut self) {
        // todo 后续要将 spi上电占用状态取消，只在发送时占用
        debug!("e-paper turn on...");
        self.pw_pin.set_high(); // 上电
        self.reset(); // 初始化
        self.send_cmd(0xD2);
        self.send_data(0x3F);
        self.send_cmd(0x00);
        self.send_data(0x6F); //# from outside
        self.send_cmd(0x01); //# power setting
        self.send_data(0x03);
        self.send_data(0x00);
        self.send_data(0x2b);
        self.send_data(0x2b);
        self.send_cmd(0x06); //# Configuring the charge pump
        self.send_data(0x3f);
        self.send_cmd(0x2A); //# Setting XON and the options of LUT
        self.send_data(0x00);
        self.send_data(0x00);
        self.send_cmd(0x30); //# Set the clock frequency
        self.send_data(0x17); //# 50Hz
        self.send_cmd(0x50); //# Set VCOM and data output interval
        self.send_data(0x57);
        self.send_cmd(0x60); //# Set The non-overlapping period of Gate and Source.
        self.send_data(0x22);
        self.send_cmd(0x61); //# resolution setting
        self.send_data(0x50); //# source 128
        self.send_data(0x80);
        self.send_cmd(0x82); //# sets VCOM_DC value
        self.send_data(0x12); //# -1v
        self.send_cmd(0xe3); //#Set POWER SAVING
        self.send_data(0x33);
        self.set_full_reg(); // reg full
        self.send_cmd(0x04); // #power on
        self.await_busy(None).unwrap(); // await
        debug!("e-paper turn on.");
    }

    /// 驱使其显示 1280byte即可控制其所有像素，w=80 h=128 共80*128=10240像素，每个像素用bit控制即可
    /// 因为灰度为2
    pub fn display(&mut self, img: Image<L>) {
        debug!("e-paper starting display...");
        self.send_cmd(0x10);
        self.send_data(BUF_BLACK_ALL);
        self.send_cmd(0x13);
        self.send_data(img_2_display_buf(&img)); // 转换并发送
        self.turn_on_display();
        debug!("e-paper started display");
    }

    /// 清屏
    pub fn clear_screen(&mut self) {
        debug!("e-paper starting clear_screen...");
        self.send_cmd(0x10);
        self.send_data(BUF_WHITE_ALL);
        self.send_cmd(0x13);
        self.send_data(BUF_BLACK_ALL);
        self.turn_on_display();
        debug!("e-paper started clear_screen");
    }

    /// 关闭连接，释放占用的引脚等，然后拉低电源引脚电压
    fn off(&mut self) {
        // todo 同 on方法一样，这里应该释放哪些占用的文件描述符
        debug!("e-paper turn off...");
        self.send_cmd(0x50);
        self.send_data(0xf7);
        self.send_cmd(0x02);
        self.await_busy(None).unwrap();
        self.send_cmd(0x07);
        self.send_data(0xA5);
        await_ms(2000);
        self.pw_pin.set_low(); // 电源off
        debug!("e-paper turn off.")
    }
}

impl Drop for Paper {
    fn drop(&mut self) {
        self.off()
    }
}


/// rppal 库 gpio使用 BCM编码，非物理编码
///
#[cfg(test)]
mod test {
    use std::thread;
    use std::time::Duration;

    use log::LevelFilter;
    use ril::{Font, Image, TextSegment};
    use ril::OverlayMode::Replace;

    use crate::pi::e_paper_ws_1in02::{BLACK_PIXEL, HEIGHT, Paper, WHITE_PIXEL, WIDTH};
    use crate::pi::font_load;

    fn log_init() {
        _ = env_logger::builder()
            .filter_level(LevelFilter::Debug)
            .is_test(true).try_init();
    }

    #[test]
    fn test_display() {
        log_init();
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
    }
}