use std::fmt;
use std::io::{self, Read, Write};

pub trait ReadLine {
    fn read_line(&mut self, buf: &mut String) -> io::Result<usize>;
}

impl ReadLine for io::Stdin {
    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        io::Stdin::read_line(self, buf)
    }
}

impl<R: io::Read> ReadLine for io::BufReader<R> {
    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        <Self as io::BufRead>::read_line(self, buf)
    }
}

impl<T: AsRef<[u8]>> ReadLine for io::Cursor<T> {
    fn read_line(&mut self, buf: &mut String) -> io::Result<usize> {
        <Self as io::BufRead>::read_line(self, buf)
    }
}

pub enum LineBufferState<T> {
    UnknownState {
        initial_line: bool,
    },
    AfterReadLine {
        line_buffer: Vec<T>,
        line_position: usize,
        line_no_more: bool,
    },
    Eof,
}

pub enum BlockBufferState<T> {
    UnknownState,
    AfterReadBlock {
        bytes_block_buffer: Box<[u8]>,
        bytes_avail_length: usize,
        bytes_position: usize,
        bytes_buffer: Option<T>,
    },
    Eof,
}

pub enum FileState<T> {
    Undefined,
    GenerationMode {
        write_buffer: Option<T>,
        write_target: Box<dyn Write>,
    },
    LineInspectionMode {
        read_line_buffer: LineBufferState<T>,
        read_target: Box<dyn ReadLine>,
        read_flag_extra_eoln_line: bool,
    },
    BlockInspectionMode {
        read_block_buffer: BlockBufferState<T>,
        read_target: Box<dyn Read>,
    },
}

impl<T> Default for FileState<T> {
    fn default() -> Self {
        FileState::Undefined
    }
}

impl<T> FileState<T> {
    fn discard_buffer_variable_value_and_get_write_target(&mut self) -> &mut dyn Write {
        match self {
            FileState::GenerationMode {
                write_buffer,
                write_target,
            } => {
                *write_buffer = None;
                write_target.as_mut()
            }
            _ => {
                panic!("file not in generation mode!");
            }
        }
    }

    fn refill<F>(&mut self)
    where
        F: PascalFile<Unit = T>,
    {
        match self {
            FileState::LineInspectionMode {
                read_line_buffer,
                read_target,
                ..
            } => match read_line_buffer {
                LineBufferState::UnknownState { initial_line } => {
                    let initial_line = *initial_line;
                    let mut buf = String::new();
                    read_target.read_line(&mut buf).expect("read line failure");
                    if initial_line && buf.is_empty() {
                        *read_line_buffer = LineBufferState::Eof;
                        return;
                    }
                    let mut line_chars = vec![];
                    F::convert_line_string_crlf_to_lf(&mut buf);
                    F::convert_line_string_to_units(&buf, &mut line_chars);
                    let no_more = match line_chars.last() {
                        Some(c) => !F::is_eoln_unit(c),
                        None => true,
                    };
                    if no_more {
                        line_chars.push(F::eoln_unit());
                    }
                    *read_line_buffer = LineBufferState::AfterReadLine {
                        line_buffer: line_chars,
                        line_position: 0,
                        line_no_more: no_more,
                    };
                }
                _ => unreachable!(),
            },
            FileState::BlockInspectionMode {
                read_block_buffer,
                read_target,
            } => {
                const IDEAL_BUFSIZE: usize = 512;
                let size_of_t = core::mem::size_of::<T>();
                assert!(size_of_t > 0);
                if matches!(read_block_buffer, BlockBufferState::UnknownState) {
                    let dest_size = ((IDEAL_BUFSIZE / size_of_t) + 1) * size_of_t;
                    *read_block_buffer = BlockBufferState::AfterReadBlock {
                        bytes_block_buffer: vec![0u8; dest_size].into_boxed_slice(),
                        bytes_avail_length: dest_size,
                        bytes_position: dest_size - size_of_t,
                        bytes_buffer: None,
                    }
                }
                match read_block_buffer {
                    BlockBufferState::AfterReadBlock {
                        bytes_block_buffer,
                        bytes_avail_length,
                        bytes_position,
                        bytes_buffer,
                    } => {
                        let bytes_position_end = *bytes_position + size_of_t;
                        let mut remaining_range = bytes_position_end..*bytes_avail_length;
                        if remaining_range.start > 0 {
                            if !remaining_range.is_empty() {
                                bytes_block_buffer.copy_within(remaining_range.clone(), 0);
                                remaining_range = 0..remaining_range.len();
                            } else {
                                remaining_range = 0..0;
                            }
                        }
                        *bytes_avail_length = remaining_range.end;
                        *bytes_position = 0;
                        *bytes_buffer = None;
                        while *bytes_avail_length < size_of_t {
                            let fillable_range = *bytes_avail_length..bytes_block_buffer.len();
                            let newly_read_len = read_target
                                .read(&mut bytes_block_buffer[fillable_range])
                                .expect("read block failure");
                            if newly_read_len == 0 {
                                *read_block_buffer = BlockBufferState::Eof;
                                return;
                            }
                            *bytes_avail_length += newly_read_len;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            _ => {
                panic!("file not in inspection mode!");
            }
        }
    }
}

pub trait PascalFile {
    type Unit;

    fn is_text_file() -> bool;

    fn is_eoln_unit(unit: &Self::Unit) -> bool;

    fn eoln_unit() -> Self::Unit;

    fn open_text_file_for_read(path: &str) -> Result<(Box<dyn ReadLine>, bool), usize>;

    fn open_binary_file_for_read(path: &str) -> Result<Box<dyn Read>, usize>;

    fn open_file_for_write(path: &str) -> Result<Box<dyn Write>, usize>;

    fn convert_line_string_crlf_to_lf(input: &mut String);

    fn convert_line_string_to_units(input: &str, units: &mut Vec<Self::Unit>);

    fn convert_blob_to_unit(input: &[u8]) -> Self::Unit;

    fn convert_unit_to_blob(data: Self::Unit, f: &mut dyn for<'a> FnMut(&'a [u8]));

    fn file_state(&self) -> &FileState<Self::Unit>;

    fn file_state_mut(&mut self) -> &mut FileState<Self::Unit>;

    fn error_state(&self) -> usize;

    fn set_error_state(&mut self, error_state: usize);
}

pub trait FromBlob {
    fn from_blob(data: &[u8]) -> Self;
}
pub trait ToBlob {
    type BlobType: core::borrow::Borrow<[u8]>;

    fn to_blob(&self) -> Self::BlobType;
}

pub fn reset<F: PascalFile + fmt::Debug, P: Into<String> + fmt::Debug>(
    file: &mut F,
    path: P,
    _options: &str,
) {
    let path = path.into();
    if F::is_text_file() {
        match F::open_text_file_for_read(&path) {
            Ok((read_target, is_terminal)) => {
                if is_terminal {
                    *file.file_state_mut() = FileState::LineInspectionMode {
                        read_target,
                        read_line_buffer: LineBufferState::AfterReadLine {
                            line_buffer: vec![F::eoln_unit()],
                            line_position: 0,
                            line_no_more: false,
                        },
                        read_flag_extra_eoln_line: true,
                    };
                } else {
                    *file.file_state_mut() = FileState::LineInspectionMode {
                        read_target,
                        read_line_buffer: LineBufferState::UnknownState { initial_line: true },
                        read_flag_extra_eoln_line: false,
                    };
                }
                file.set_error_state(0);
            }
            Err(e) => {
                *file.file_state_mut() = FileState::Undefined;
                file.set_error_state(e);
            }
        }
    } else {
        match F::open_binary_file_for_read(&path) {
            Ok(read_target) => {
                *file.file_state_mut() = FileState::BlockInspectionMode {
                    read_target,
                    read_block_buffer: BlockBufferState::UnknownState,
                };
                file.set_error_state(0);
            }
            Err(e) => {
                *file.file_state_mut() = FileState::Undefined;
                file.set_error_state(e);
            }
        }
    }
}

pub fn rewrite<F: PascalFile, P: Into<String>>(file: &mut F, path: P, _options: &str) {
    let path = path.into();
    match F::open_file_for_write(&path) {
        Ok(write_target) => {
            *file.file_state_mut() = FileState::GenerationMode {
                write_target,
                write_buffer: None,
            };
            file.set_error_state(0);
        }
        Err(e) => {
            *file.file_state_mut() = FileState::Undefined;
            file.set_error_state(e);
        }
    }
}

pub fn buffer_variable_assign<F: PascalFile>(file: &mut F, value: F::Unit) {
    match file.file_state_mut() {
        FileState::GenerationMode { write_buffer, .. } => {
            *write_buffer = Some(value);
        }
        _ => {
            panic!("file not in generation mode!");
        }
    }
}

pub fn put<F: PascalFile>(file: &mut F) {
    match file.file_state_mut() {
        FileState::GenerationMode {
            write_target,
            write_buffer,
        } => {
            let caret_value = write_buffer
                .take()
                .expect("file buffer variable value is undefined!");
            F::convert_unit_to_blob(caret_value, &mut |data| {
                write_target.write_all(data).expect("fail to write data");
            });
        }
        _ => {
            panic!("file not in generation mode!");
        }
    }
}

pub fn get<F: PascalFile>(file: &mut F) {
    match file.file_state_mut() {
        FileState::LineInspectionMode {
            read_line_buffer, ..
        } => match read_line_buffer {
            LineBufferState::Eof => {
                panic!("file eof reached");
            }
            LineBufferState::UnknownState { .. } => {
                file.file_state_mut().refill::<F>();
            }
            LineBufferState::AfterReadLine {
                line_buffer,
                line_position,
                line_no_more,
            } => {
                if *line_position + 1 < line_buffer.len() {
                    *line_position += 1;
                } else if *line_no_more {
                    *read_line_buffer = LineBufferState::Eof
                } else {
                    *read_line_buffer = LineBufferState::UnknownState {
                        initial_line: false,
                    };
                }
            }
        },
        FileState::BlockInspectionMode {
            read_block_buffer, ..
        } => match read_block_buffer {
            BlockBufferState::Eof => {
                panic!("file eof reached");
            }
            BlockBufferState::UnknownState => {
                file.file_state_mut().refill::<F>();
            }
            BlockBufferState::AfterReadBlock {
                bytes_avail_length,
                bytes_position,
                bytes_buffer,
                ..
            } => {
                let size_of_t = core::mem::size_of::<F::Unit>();
                assert!(size_of_t > 0);
                let bytes_position_end = *bytes_position + size_of_t;
                let new_bytes_position_end = bytes_position_end + size_of_t;
                if new_bytes_position_end > *bytes_avail_length {
                    file.file_state_mut().refill::<F>();
                    return;
                }
                *bytes_buffer = None;
                *bytes_position = bytes_position_end;
            }
        },
        _ => {
            panic!("file not in inspection mode");
        }
    }
}

pub fn buffer_variable<F: PascalFile>(file: &mut F) -> F::Unit
where
    F::Unit: Clone,
{
    loop {
        match file.file_state_mut() {
            FileState::LineInspectionMode {
                read_line_buffer, ..
            } => match read_line_buffer {
                LineBufferState::Eof => {
                    panic!("file eof reached");
                }
                LineBufferState::UnknownState { .. } => {
                    file.file_state_mut().refill::<F>();
                    continue;
                }
                LineBufferState::AfterReadLine {
                    line_buffer,
                    line_position,
                    ..
                } => {
                    return line_buffer[*line_position].clone();
                }
            },
            FileState::BlockInspectionMode {
                read_block_buffer, ..
            } => match read_block_buffer {
                BlockBufferState::Eof => {
                    panic!("file eof reached");
                }
                BlockBufferState::UnknownState => {
                    file.file_state_mut().refill::<F>();
                    continue;
                }
                BlockBufferState::AfterReadBlock {
                    bytes_block_buffer,
                    bytes_buffer,
                    bytes_position,
                    ..
                } => match bytes_buffer {
                    None => {
                        let size_of_t = core::mem::size_of::<F::Unit>();
                        assert!(size_of_t > 0);
                        let bytes_position_end = *bytes_position + size_of_t;
                        let v = F::convert_blob_to_unit(
                            &bytes_block_buffer[*bytes_position..bytes_position_end],
                        );
                        *bytes_buffer = Some(v.clone());
                        return v;
                    }
                    Some(v) => {
                        return v.clone();
                    }
                },
            },
            _ => panic!("file not in inspection mode"),
        }
    }
}

pub fn eof<F: PascalFile>(file: &mut F) -> bool {
    loop {
        match file.file_state() {
            FileState::LineInspectionMode {
                read_line_buffer, ..
            } => match read_line_buffer {
                LineBufferState::Eof => {
                    return true;
                }
                LineBufferState::UnknownState { .. } => {
                    file.file_state_mut().refill::<F>();
                    continue;
                }
                LineBufferState::AfterReadLine { .. } => {
                    return false;
                }
            },
            FileState::BlockInspectionMode {
                read_block_buffer, ..
            } => match read_block_buffer {
                BlockBufferState::Eof => {
                    return true;
                }
                BlockBufferState::UnknownState { .. } => {
                    file.file_state_mut().refill::<F>();
                    continue;
                }
                BlockBufferState::AfterReadBlock { .. } => {
                    return false;
                }
            },
            FileState::GenerationMode { .. } => {
                return true;
            }
            _ => panic!("file not in any mode"),
        }
    }
}

pub fn eoln<F: PascalFile>(file: &mut F) -> bool {
    loop {
        match file.file_state() {
            FileState::LineInspectionMode {
                read_line_buffer, ..
            } => match read_line_buffer {
                LineBufferState::Eof => {
                    panic!("file eof reached");
                }
                LineBufferState::UnknownState { .. } => {
                    file.file_state_mut().refill::<F>();
                    continue;
                }
                LineBufferState::AfterReadLine {
                    line_buffer,
                    line_position,
                    ..
                } => {
                    return F::is_eoln_unit(&line_buffer[*line_position]);
                }
            },
            FileState::BlockInspectionMode { .. } => panic!("file is not text file"),
            _ => panic!("file not in inspection mode"),
        }
    }
}

pub fn write<F: PascalFile, T: fmt::Display>(file: &mut F, val: T) {
    let write_target = file
        .file_state_mut()
        .discard_buffer_variable_value_and_get_write_target();
    write!(write_target, "{}", val).unwrap();
}

pub fn write_ln<F: PascalFile, T: fmt::Display>(file: &mut F, val: T) {
    let write_target = file
        .file_state_mut()
        .discard_buffer_variable_value_and_get_write_target();
    writeln!(write_target, "{}", val).unwrap();
}

pub fn write_ln_noargs<F: PascalFile>(file: &mut F) {
    let write_target = file
        .file_state_mut()
        .discard_buffer_variable_value_and_get_write_target();
    writeln!(write_target).unwrap();
}

pub fn write_binary<F: PascalFile, T: ToBlob>(file: &mut F, val: T) {
    use core::borrow::Borrow;
    let write_target = file
        .file_state_mut()
        .discard_buffer_variable_value_and_get_write_target();
    let blob = val.to_blob();
    write_target.write_all(blob.borrow()).unwrap();
}

pub fn r#break<F: PascalFile>(file: &mut F) {
    let write_target = file
        .file_state_mut()
        .discard_buffer_variable_value_and_get_write_target();
    write_target.flush().unwrap();
}

pub fn read_onearg<F: PascalFile>(file: &mut F) -> F::Unit
where
    F::Unit: Copy,
{
    let v = buffer_variable(file);
    get(file);
    v
}

pub fn read_ln<F: PascalFile>(file: &mut F) {
    while !eoln(file) {
        get(file);
    }
    get(file);
}

pub fn break_in<F: PascalFile>(file: &mut F, _: bool) {
    // FIXME: this seems nonstandard. Verify if this handling is correct.
    // and not sure what the 2nd argument should do here.
    let file_state = file.file_state_mut();
    match file_state {
        FileState::LineInspectionMode {
            read_line_buffer,
            read_flag_extra_eoln_line,
            ..
        } => match read_line_buffer {
            LineBufferState::AfterReadLine { line_no_more, .. } => {
                if *line_no_more {
                    *read_line_buffer = LineBufferState::Eof
                } else if *read_flag_extra_eoln_line {
                    *read_line_buffer = LineBufferState::AfterReadLine {
                        line_buffer: vec![F::eoln_unit()],
                        line_position: 0,
                        line_no_more: false,
                    };
                } else {
                    *read_line_buffer = LineBufferState::UnknownState {
                        initial_line: false,
                    };
                }
            }
            LineBufferState::Eof | LineBufferState::UnknownState { .. } => {}
        },
        _ => panic!("file is not in line-inspection mode"),
    }
}

pub fn erstat<F: PascalFile>(file: &mut F) -> usize {
    file.error_state()
}

pub fn close<F: PascalFile>(file: &mut F) {
    *file.file_state_mut() = FileState::default();
}

impl FromBlob for u8 {
    fn from_blob(data: &[u8]) -> Self {
        assert!(data.len() == 1);
        data[0]
    }
}

impl ToBlob for u8 {
    type BlobType = [u8; 1];

    fn to_blob(&self) -> Self::BlobType {
        [*self]
    }
}
