// Copyright Amazon.com, Inc. or its affiliates. All Rights Reserved.
// SPDX-License-Identifier: Apache-2.0
use llrt_encoding::{
    bytes_to_b64_string, bytes_to_hex_string, bytes_to_utf16_string, Encoder, Endian,
};
use llrt_utils::{
    bytes::ObjectBytes,
    module::{export_default, ModuleInfo},
    result::ResultExt,
};
use rquickjs::{
    class::Trace,
    module::{Declarations, Exports, ModuleDef},
    prelude::Opt,
    Class, Ctx, Exception, Result, Value,
};

#[rquickjs::class]
#[derive(Clone, Trace, rquickjs::JsLifetime)]
pub struct StringDecoder {
    #[qjs(skip_trace)]
    encoder: Encoder,
    #[qjs(skip_trace)]
    buffered_bytes: Vec<u8>,
}

#[rquickjs::methods]
impl<'js> StringDecoder {
    #[qjs(constructor)]
    pub fn new(ctx: Ctx<'js>, encoding: Opt<String>) -> Result<Self> {
        Ok(StringDecoder {
            encoder: Encoder::from_optional_str(encoding.as_deref()).or_throw(&ctx)?,
            buffered_bytes: Vec::new(),
        })
    }

    #[qjs(skip)]
    fn required_utf8_length(&self, first_byte: u8) -> usize {
        match first_byte {
            0x00..=0x7F => 1, // 0xxxxxxx
            0xC2..=0xDF => 2, // 110xxxxx
            0xE0..=0xEF => 3, // 1110xxxx
            0xF0..=0xF7 => 4, // 11110xxx
            _ => 1,           // Treat invalid lead bytes as a single invalid byte
        }
    }

    #[qjs(skip)]
    fn consume_valid_utf8_prefix(&mut self, valid_up_to: usize) -> String {
        let prefix = simdutf8::basic::from_utf8(&self.buffered_bytes[..valid_up_to])
            .expect("valid_up_to guarantees this slice is valid UTF-8")
            .to_owned();
        self.buffered_bytes.drain(..valid_up_to);
        prefix
    }

    /// Checks if the bytes after the first are all valid continuation bytes (10xxxxxx)
    #[qjs(skip)]
    fn has_valid_utf8_continuations(&self) -> bool {
        self.buffered_bytes[1..].iter().all(|&b| (b & 0xC0) == 0x80)
    }

    #[qjs(skip)]
    fn decode_utf8(&mut self, bytes: &[u8]) -> Result<String> {
        self.buffered_bytes.extend_from_slice(bytes);
        let mut result = String::new();

        loop {
            match simdutf8::compat::from_utf8(&self.buffered_bytes) {
                Ok(valid_str) => {
                    result.push_str(valid_str);
                    self.buffered_bytes.clear();
                    break;
                },
                Err(err) => {
                    let valid_up_to = err.valid_up_to();
                    if valid_up_to > 0 {
                        result.push_str(&self.consume_valid_utf8_prefix(valid_up_to));
                        continue;
                    } else {
                        let required_bytes = self.required_utf8_length(self.buffered_bytes[0]);
                        if self.buffered_bytes.len() < required_bytes {
                            // If we have more than one byte and the next bytes are not valid continuation bytes
                            // then treat the first byte as invalid
                            if self.buffered_bytes.len() > 1 && !self.has_valid_utf8_continuations()
                            {
                                result.push('\u{FFFD}');
                                self.buffered_bytes.drain(..1);
                                continue;
                            } else {
                                // Wait for  more bytes to arrive that could potentially complete the code point
                                break;
                            }
                        } else if let Some(invalid_len) = err.error_len() {
                            // We have a complete invalid sequence, push a replacement character
                            result.push('\u{FFFD}');
                            self.buffered_bytes.drain(..invalid_len);
                            continue;
                        } else {
                            // We shouldn't ever reach here as this case should be handled when
                            // self.buffered_bytes.len() < required_bytes
                            break;
                        }
                    }
                },
            }
        }
        Ok(result)
    }

    #[qjs(skip)]
    fn decode_base64(&mut self, bytes: &[u8]) -> Result<String> {
        self.buffered_bytes.extend_from_slice(bytes);
        let mut output = String::new();

        // Base64 encoding uses blocks of 3 bytes (representing 4 characters)
        let decodable_len = (self.buffered_bytes.len() / 3) * 3;
        if decodable_len > 0 {
            output.push_str(&bytes_to_b64_string(&self.buffered_bytes[..decodable_len]));
            self.buffered_bytes.drain(..decodable_len);
        }
        Ok(output)
    }

    #[qjs(skip)]
    fn get_utf16_decodable_len(&self, endian: Endian) -> usize {
        let mut decodable_len = (self.buffered_bytes.len() / 2) * 2;
        // If there are at least 2 bytes, check if the last code unit is an unpaired high surrogate
        if decodable_len >= 2 {
            let last_unit = match endian {
                Endian::Little => u16::from_le_bytes([
                    self.buffered_bytes[decodable_len - 2],
                    self.buffered_bytes[decodable_len - 1],
                ]),
                Endian::Big => u16::from_be_bytes([
                    self.buffered_bytes[decodable_len - 2],
                    self.buffered_bytes[decodable_len - 1],
                ]),
            };
            // If the last code unit is a high surrogate, it might be incomplete
            if (0xD800..=0xDBFF).contains(&last_unit) {
                decodable_len -= 2;
            }
        }
        decodable_len
    }

    #[qjs(skip)]
    fn decode_utf16(&mut self, ctx: &Ctx<'js>, bytes: &[u8], endian: Endian) -> Result<String> {
        self.buffered_bytes.extend_from_slice(bytes);
        let decodable_len = self.get_utf16_decodable_len(endian);
        if decodable_len == 0 {
            return Ok(String::new());
        }
        let output = bytes_to_utf16_string(&self.buffered_bytes[..decodable_len], endian, true)
            .map_err(|_| Exception::throw_message(&ctx, "Error decoding UTF16 string"));

        self.buffered_bytes.drain(..decodable_len);
        output
    }

    #[qjs(skip)]
    fn flush_utf16(&self, ctx: &Ctx<'js>, endian: Endian) -> Result<String> {
        let decodable_len = self.get_utf16_decodable_len(endian);
        let mut result = bytes_to_utf16_string(&self.buffered_bytes[..decodable_len], endian, true)
            .map_err(|_| Exception::throw_message(ctx, "Error flushing string"))?;
        if decodable_len != self.buffered_bytes.len() {
            result.push('\u{FFFD}');
        }
        Ok(result)
    }

    #[qjs(skip)]
    fn flush(&mut self, ctx: &Ctx<'js>) -> Result<String> {
        if self.buffered_bytes.is_empty() {
            return Ok(String::new());
        }

        let out = match self.encoder {
            Encoder::Hex => Ok(bytes_to_hex_string(&self.buffered_bytes)),
            Encoder::Base64 => Ok(bytes_to_b64_string(&self.buffered_bytes)),
            Encoder::Utf8 | Encoder::Windows1252 => {
                Ok(String::from_utf8_lossy(&self.buffered_bytes).to_string())
            },
            Encoder::Utf16le => self.flush_utf16(ctx, Endian::Little),
            Encoder::Utf16be => self.flush_utf16(ctx, Endian::Big),
        };

        self.buffered_bytes.clear();
        out
    }

    #[qjs(skip)]
    fn decode_bytes(&mut self, ctx: &Ctx<'js>, bytes: &[u8]) -> Result<String> {
        match self.encoder {
            Encoder::Hex => Ok(bytes_to_hex_string(bytes)),
            Encoder::Base64 => self.decode_base64(bytes),
            Encoder::Utf8 | Encoder::Windows1252 => self.decode_utf8(bytes),
            Encoder::Utf16le => self.decode_utf16(ctx, bytes, Endian::Little),
            Encoder::Utf16be => self.decode_utf16(ctx, bytes, Endian::Big),
        }
    }

    pub fn write(&mut self, ctx: Ctx<'js>, input: Value<'js>) -> Result<String> {
        let object_bytes = ObjectBytes::from(&ctx, &input)?;
        let bytes = object_bytes.as_bytes(&ctx)?;

        self.decode_bytes(&ctx, bytes)
    }

    pub fn end(&mut self, ctx: Ctx<'js>, input: Opt<Value<'js>>) -> Result<String> {
        let mut result = String::new();

        if let Some(value) = input.0 {
            let object_bytes = ObjectBytes::from(&ctx, &value)?;
            let bytes = object_bytes.as_bytes(&ctx)?;
            result.push_str(&self.decode_bytes(&ctx, bytes)?);
        }

        // Flush any remaining buffered_bytes
        result.push_str(&self.flush(&ctx)?);

        Ok(result)
    }
}

pub struct StringDecoderModule;

impl ModuleDef for StringDecoderModule {
    fn declare(declare: &Declarations) -> Result<()> {
        declare.declare(stringify!(StringDecoder))?;
        declare.declare("default")?;
        Ok(())
    }

    fn evaluate<'js>(ctx: &Ctx<'js>, exports: &Exports<'js>) -> Result<()> {
        export_default(ctx, exports, |default| {
            Class::<StringDecoder>::define(default)?;
            Ok(())
        })
    }
}

impl From<StringDecoderModule> for ModuleInfo<StringDecoderModule> {
    fn from(val: StringDecoderModule) -> Self {
        ModuleInfo {
            name: "string_decoder",
            module: val,
        }
    }
}
