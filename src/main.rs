#![no_std] // don't link the Rust standard library
#![no_main] // disable all Rust-level entry points
#![feature(custom_test_frameworks)]
#![test_runner(crate::test::test_runner)]
#![reexport_test_harness_main = "test_main"]

use core::panic::PanicInfo;

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}

#[macro_export]
macro_rules! print_out {
    ($($arg:tt)*) => {
        $crate::port_io::print_out(format_args!($($arg)*));
    };
}

#[macro_export]
macro_rules! println_out {
() => ($crate::print_out!("\n"));
($fmt:expr) => ($crate::print_out!(concat!($fmt, "\n")));
($fmt:expr, $($arg:tt)*) => ($crate::print_out!(
    concat!($fmt, "\n"), $($arg)*));
}

mod vga_buffer {
    use core::fmt;
    use lazy_static::lazy_static;
    use spin::Mutex;
    use volatile::Volatile;

    #[allow(dead_code)]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum Color {
        Black = 0,
        Blue = 1,
        Green = 2,
        Cyan = 3,
        Red = 4,
        Magenta = 5,
        Brown = 6,
        LightGray = 7,
        DarkGray = 8,
        LightBlue = 9,
        LightGreen = 10,
        LightCyan = 11,
        LightRed = 12,
        Pink = 13,
        Yellow = 14,
        White = 15,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(transparent)]
    struct ColorCode(u8);

    impl ColorCode {
        fn new(foreground: Color, background: Color) -> ColorCode {
            ColorCode((background as u8) << 4 | (foreground as u8))
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(C)]
    struct ScreenChar {
        ascii_character: u8,
        color_code: ColorCode,
    }

    const BUFFER_HEIGHT: usize = 25;
    const BUFFER_WIDTH: usize = 80;

    #[repr(transparent)]
    struct Buffer {
        chars: [[Volatile<ScreenChar>; BUFFER_WIDTH]; BUFFER_HEIGHT],
    }

    pub struct Writer {
        column_position: usize,
        row_position: usize,
        color_code: ColorCode,
        buffer: &'static mut Buffer,
    }

    impl Writer {
        pub fn write_byte(&mut self, byte: u8) {
            match byte {
                b'\n' => self.new_line(),
                byte => {
                    if self.column_position >= BUFFER_WIDTH {
                        self.new_line();
                    }

                    let row = self.row_position;
                    let col = self.column_position;

                    let color_code = self.color_code;
                    self.buffer.chars[row][col].write(ScreenChar {
                        ascii_character: byte,
                        color_code,
                    });
                    self.column_position += 1;
                }
            }
        }

        fn new_line(&mut self) {
            self.row_position += 1;
            self.column_position = 0;
        }

        pub fn write_string(&mut self, s: &str) {
            for byte in s.bytes() {
                match byte {
                    // printable ASCII byte or newline
                    0x20..=0x7e | b'\n' => self.write_byte(byte),
                    // not part of printable ASCII range
                    _ => self.write_byte(0xfe),
                }
            }
        }
    }

    impl fmt::Write for Writer {
        fn write_str(&mut self, s: &str) -> fmt::Result {
            self.write_string(s);
            Ok(())
        }
    }

    lazy_static! {
        pub static ref WRITER: Mutex<Writer> = Mutex::new(Writer {
            column_position: 0,
            row_position: 0,
            color_code: ColorCode::new(Color::White, Color::Black),
            buffer: unsafe { &mut *(0xb8000 as *mut Buffer) },
        });
    }

    #[doc(hidden)]
    pub fn _print(args: fmt::Arguments) {
        use core::fmt::Write;
        WRITER.lock().write_fmt(args).unwrap();
    }

    mod tests {
        use super::*;

        #[test_case]
        fn it_can_println() {
            let s = "Some test string that fits on a single line";
            println!("{}", s);
            for (i, c) in s.chars().enumerate() {
                let screen_char = WRITER.lock().buffer.chars[0][i].read();
                assert_eq!(char::from(screen_char.ascii_character), c);
            }
        }
    }
}

mod port_io {
    use lazy_static::lazy_static;
    use spin::Mutex;
    use uart_16550::SerialPort;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u32)]
    pub enum QemuExitCode {
        Success = 0x10,
        Failed = 0x11,
    }

    pub fn exit_qemu(exit_code: QemuExitCode) {
        use x86_64::instructions::port::Port;

        unsafe {
            let mut port = Port::new(0xf4);
            port.write(exit_code as u32);
        }
    }

    #[doc(hidden)]
    pub fn print_out(args: ::core::fmt::Arguments) {
        use core::fmt::Write;
        SERIAL1
            .lock()
            .write_fmt(args)
            .expect("Printing to serial failed");
    }

    lazy_static! {
        pub static ref SERIAL1: Mutex<SerialPort> = {
            let mut serial_port = unsafe { SerialPort::new(0x3F8) };
            serial_port.init();
            Mutex::new(serial_port)
        };
    }
}

mod test {
    use crate::port_io;

    #[cfg(test)]
    pub fn test_runner(tests: &[&dyn Testable]) {
        print_out!("\x1B[2J\x1B[1;1H");
        println_out!(
            "Running {} {}",
            tests.len(),
            match tests.len() {
                1 => "test",
                _ => "tests",
            }
        );
        for test in tests {
            test.run();
        }
        port_io::exit_qemu(port_io::QemuExitCode::Success);
    }

    pub trait Testable {
        fn run(&self) -> ();
    }

    impl<T> Testable for T
    where
        T: Fn(),
    {
        fn run(&self) {
            print_out!("{}...\t", core::any::type_name::<T>());
            self();
            println_out!("[ok]");
        }
    }
}

#[no_mangle] // don't mangle the name of this function
pub extern "C" fn _start() -> ! {
    #[cfg(test)]
    test_main();

    loop {}
}

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println_out!("[failed]\n");
    println_out!("Error: {}\n", info);
    port_io::exit_qemu(port_io::QemuExitCode::Failed);
    loop {}
}
