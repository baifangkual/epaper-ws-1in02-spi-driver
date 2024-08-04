use std::ops::Add;
use std::time::{Duration, SystemTime};
use ab_glyph::{FontRef, PxScale};
use image::{ImageBuffer, Luma};
use imageproc::definitions::Image;
use imageproc::drawing::{draw_text_mut, text_size};
use log::{debug, error, info, log_enabled, warn};
use log::Level::Debug;
use rppal::gpio::{Gpio, InputPin, Level, OutputPin};
use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
use crate::pi::buf_type_impl::Bytes;
use crate::pi::{font_load, pi_config};

/// 高（因为是竖向一字节八个像素
pub const WIDTH: u32 = 80;
/// 宽（因为是竖向一字节八个像素
pub const HEIGHT: u32 = 128;
/// 轮询忙状态的间隔
const AWAIT_BUSY_MS: i32 = 50;
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


/// 使当前线程sleep等待一定毫秒数，因为是sleep，所以不一定准确，该偏差属可接受范围内
fn await_ms(ms: i32) {
    std::thread::sleep(Duration::from_millis(ms as u64))
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
            Ok(s_size) => {
                if s_size > 1 && log_enabled!(Debug) {
                    debug!("spi send buf size: {s_size}")
                }
            }
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
                        await_ms(AWAIT_BUSY_MS);
                    }
                }
                None => {
                    loop {
                        if self.on_busy() {
                            await_ms(AWAIT_BUSY_MS);
                        } else { break; }
                    }
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
        for i in 0..42_usize {
            self.send_data(FULL_LUT_W_REG_DATA[i])
        }
        self.send_cmd(CMD_LUT_B_REG);
        for i in 0..42_usize {
            self.send_data(FULL_LUT_B_REG_DATA[i])
        }
    }
    /// 向其发送lut涌动数据 局部刷新的
    fn set_part_reg(&mut self) {
        self.send_cmd(CMD_LUT_W_REG);
        for i in 0..42_usize {
            self.send_data(PART_LUT_W_REG_DATA[i])
        }
        self.send_cmd(CMD_LUT_B_REG);
        for i in 0..42_usize {
            self.send_data(PART_LUT_B_REG_DATA[i])
        }
    }

    /// 初始化墨水品
    pub fn on(&mut self) {

        // todo 后续要将 spi上电占用状态取消，只在发送时占用

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

        self.await_busy(None).unwrap() // await
    }

    /// 驱使其显示 1280byte即可控制其所有像素，w=80 h=128 共80*128=10240像素，每个像素用bit控制即可
    /// 因为灰度为2
    pub fn display<T: Bytes>(&mut self, img: T) {
        // self.send_command(0x10)
        // for j in range(0, self.height):
        // for i in range(0, int(Width)):
        //     self.send_data(0xff)
        //
        // self.send_command(0x13)
        // for j in range(0, self.height):
        // for i in range(0, int(Width)):
        //     self.send_data(image[i + j * int(Width)])
        // self.TurnOnDisplay()
        let font = FontRef::try_from_slice(font_load::FONT).unwrap();
        let f_px = PxScale::from(12.0_f32);
        // luma 灰度图片 255 默认全白
        let mut image = ImageBuffer::from_pixel(HEIGHT, WIDTH, Luma([255u8]));
        draw_text_mut(&mut image, Luma([0u8]), 2, 0, f_px, &font, "1234abcd");

        //应该需要行列变换
        //todo


        let di = image.into_raw();
        let dis = di.len();
        debug!("di size: {dis}");

        self.send_cmd(0x10);
        for _ in 0..WIDTH * HEIGHT / 8 {
            self.send_data(0xff_u8);
        }
        self.send_cmd(0x13);

        for x in di.chunks(1280) {
            self.send_data(x)
        }

        // let w =  WIDTH as usize;
        // for j in (0.. HEIGHT) {
        //     for i in 0.. WIDTH as usize {
        //         self.send_data(&[img[i + j * w]])
        //     }
        // }
        //
        // self.send_data(di);
        self.turn_on_display();
    }

    /// 清屏
    pub fn clear_screen(&mut self) {
        self.send_cmd(0x10);
        for _ in 0..WIDTH * HEIGHT / 8 {
            self.send_data(0x00_u8);
        }
        self.send_cmd(0x13);
        for _ in 0..WIDTH * HEIGHT / 8 {
            self.send_data(0xff_u8);
        }
        self.turn_on_display();
    }

    pub fn off(&mut self) {

        // todo 同 on方法一样，这里应该释放哪些占用的文件描述符
        /*
        self.send_command(0x50)
        self.send_data(0xf7)
        self.send_command(0x02)
        self.ReadBusy()
        self.send_command(0x07)
        self.send_data(0xA5)
        epd_endpoint.delay_ms(200)
        epd_endpoint.delay_ms(2000)
        epd_endpoint.module_exit()
         */
        self.send_cmd(0x50);
        self.send_data(0xf7);
        self.send_cmd(0x02);
        self.await_busy(None).unwrap();
        self.send_cmd(0x07);
        self.send_data(0xA5);
        await_ms(2000);
        self.pw_pin.set_low(); // 电源off
    }
}


/// rppal 库 gpio使用 BCM编码，非物理编码
///
#[cfg(test)]
mod test {
    use std::thread;
    use std::time::Duration;

    use log::{info, LevelFilter};
    use rppal::gpio::{Gpio};
    use rppal::spi::{Bus, Mode, SlaveSelect, Spi};
    use crate::pi::e_paper_ws_1in02::Paper;

    fn log_init() {
        _ = env_logger::builder()
            .filter_level(LevelFilter::Debug)
            .is_test(true).try_init();
    }

    #[test]
    fn test_lib_spi_apt() {
        let mut spi = Spi::new(Bus::Spi0, SlaveSelect::Ss0, 4000000_u32, Mode::Mode0).unwrap();
        spi.write(&[1, 2, 3, 5, 4]).unwrap();
    }

    #[test]
    fn test_lib_gpio_api() {
        log_init();

        let cs_pin: u8 = 8;

        let gpio = Gpio::new().unwrap();
        let mut cs = gpio.get(cs_pin).unwrap().into_output_low();
        info!("start set high");
        cs.set_high();
        info!("end set high");
        info!("thread start sleep");
        thread::sleep(Duration::from_secs(5));
        info!("thread end sleep");
        info!("start set low");
        cs.set_low();
        info!("end set low");
        info!("thread start sleep");
        thread::sleep(Duration::from_secs(5));
        info!("thread end sleep");
        info!("end...")
    }

    #[test]
    fn test_display() {
        log_init();

        let test_data = [252_u8, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 252, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 239, 255, 255, 255, 255, 255, 255, 255, 255, 255, 239, 255, 255, 255, 255, 255, 255, 255, 255, 255, 239, 255, 255, 255, 255, 255, 255, 255, 255, 252, 31, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 248, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 159, 255, 255, 255, 255, 255, 255, 255, 255, 253, 175, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 252, 111, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 252, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 252, 1, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 252, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 252, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 252, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 224, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 252, 223, 255, 255, 255, 255, 255, 255, 255, 255, 254, 63, 255, 255, 255, 255, 255, 255, 255, 255, 255, 127, 255, 255, 255, 255, 255, 255, 255, 255, 224, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 244, 1, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 244, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 224, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 227, 255, 255, 255, 255, 255, 255, 255, 255, 252, 109, 255, 255, 255, 255, 255, 255, 255, 255, 253, 173, 255, 255, 255, 255, 255, 255, 255, 255, 253, 141, 255, 255, 255, 255, 255, 255, 255, 255, 254, 115, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 239, 255, 255, 255, 255, 255, 255, 255, 255, 255, 237, 255, 255, 255, 255, 255, 255, 255, 255, 255, 224, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 111, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 224, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 31, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 253, 239, 255, 255, 255, 255, 255, 255, 255, 255, 224, 15, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 254, 15, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 253, 111, 255, 255, 255, 255, 255, 255, 255, 255, 253, 159, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255];

        let mut paper = Paper::new();
        paper.on();
        paper.clear_screen();
        paper.display(&test_data[..]);
        thread::sleep(Duration::from_secs(2));
        paper.clear_screen();
        paper.off();
    }

    #[test]
    fn windows_test() {
        let v = vec![1, 2, 3, 4, 5, 6, 7, 8];
        for x in v.chunks(2) {
            let xl = x.len();
            println!("windows len: {}, data: {:?}", xl, x)
        }
    }
}