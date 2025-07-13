import { StringDecoder } from 'string_decoder';

/**
 * These tests are inspired by:
 * https://github.com/nodejs/node/blob/main/test/parallel/test-string-decoder.js
 */
describe('StringDecoder', () => {
  it('should default to UTF-8 decoding', () => {
    const decoder = new StringDecoder();
    expect(decoder.write(Buffer.from([0xC2, 0xA2]))).toEqual('¢');
    expect(decoder.write(Buffer.from([0xE2, 0x82, 0xAC]))).toEqual('€');
    expect(decoder.end()).toEqual('');
  });

  it('should decode HEX', () => {
    const decoder = new StringDecoder('hex');
    expect(decoder.write(Buffer.from([0x27, 0x42, 0x6c]))).toEqual('27426c');
    expect(decoder.write(Buffer.from([0xa4, 0xf1]))).toEqual('a4f1');
    expect(decoder.end()).toEqual('');
  });
});

describe('StringDecoder UTF-8', () => {
  const decoder = new StringDecoder('utf8');

  it('should decode sequences in all possible splits', () => {
    test(decoder, Buffer.from('$', 'utf8'), '$');
    test(decoder, Buffer.from('¢', 'utf8'), '¢');
    test(decoder, Buffer.from('€', 'utf8'), '€');
    test(decoder, Buffer.from('𤭢', 'utf8'), '𤭢');
    // Contains 1, 2, 3 and 4 byte characters
    test(decoder, Buffer.from('aéカ🥐', 'utf8'), 'aéカ🥐');
  });

  it('should return empty string and then U+FFFD for incomplete sequence', () => {
    expect(decoder.write(Buffer.from('E18B', 'hex'))).toEqual('');
    expect(decoder.end()).toEqual('\ufffd');
  });

  it('should return U+FFFD for standalone replacement character', () => {
    expect(decoder.write(Buffer.from('\ufffd'))).toEqual('\ufffd');
    expect(decoder.end()).toEqual('');
  });

  it('should return multiple U+FFFD characters correctly', () => {
    expect(decoder.write(Buffer.from('\ufffd\ufffd\ufffd'))).toEqual('\ufffd\ufffd\ufffd');
    expect(decoder.end()).toEqual('');
  });

  it('should return U+FFFD and then another U+FFFD for incomplete sequence', () => {
    expect(decoder.write(Buffer.from('EFBFBDE2', 'hex'))).toEqual('\ufffd');
    expect(decoder.end()).toEqual('\ufffd');
  });

  it('should handle mixed valid and invalid sequences', () => {
    expect(decoder.write(Buffer.from('F1', 'hex'))).toEqual('');
    expect(decoder.write(Buffer.from('41F2', 'hex'))).toEqual('\ufffdA');
    expect(decoder.end()).toEqual('\ufffd');
  });

  it('should provide correct number of replacement chars for incomplete multibyte sequences', () => {
    expect(decoder.write(Buffer.from('f69b', 'hex'))).toEqual('');
    expect(decoder.write(Buffer.from('d1', 'hex'))).toEqual('\ufffd\ufffd');
    expect(decoder.end()).toEqual('\ufffd');
    expect(decoder.write(Buffer.from('f4', 'hex'))).toEqual('');
    expect(decoder.write(Buffer.from('bde5', 'hex'))).toEqual('\ufffd\ufffd');
    expect(decoder.end()).toEqual('\ufffd');
  });
});

describe('StringDecoder UTF-16LE', () => {
  const decoder = new StringDecoder('utf-16le');

  it('utf-16le decoding', () => {
    expect(decoder.write(Buffer.from([0x3d, 0xd8, 0x4d, 0xdc]))).toEqual('👍');
    expect(decoder.end()).toEqual('');

    expect(decoder.write(Buffer.from([0x3d, 0xd8]))).toEqual('');
    expect(decoder.write(Buffer.from([0x4d]))).toEqual('');
    expect(decoder.write(Buffer.from([0xdc]))).toEqual('👍');
    expect(decoder.end()).toEqual('');
  });

  it('should handle split surrogate pairs in utf16le', () => {
    expect(decoder.write(Buffer.from('3DD8', 'hex'))).toEqual('');
    expect(decoder.write(Buffer.from('4D', 'hex'))).toEqual('');
    expect(decoder.write(Buffer.from('DC', 'hex'))).toEqual('\ud83d\udc4d');
    expect(decoder.end()).toEqual('');
  });

  it('should handle incomplete surrogate pairs at end in utf16le', () => {
    console.log("Ainsley was here")
    expect(decoder.write(Buffer.from('3DD8', 'hex'))).toEqual('');
    console.log("Ainsley was here2")
    expect(decoder.end()).toEqual('\ud83d');
    console.log("Ainsley was here")
  });

  it('should handle incomplete sequences with multiple writes in utf16le', () => {
    expect(decoder.write(Buffer.from('3DD8', 'hex'))).toEqual('');
    expect(decoder.write(Buffer.from('4D', 'hex'))).toEqual('');
    expect(decoder.end()).toEqual('\ud83d');
  });

  it('should handle unmatched surrogate in utf16le', () => {
    const decoder = new StringDecoder('utf-16le');
    expect(decoder.write(Buffer.from('3DD84D', 'hex'))).toEqual('\ud83d');
    expect(decoder.end()).toEqual('');
  });
});

// TODO: write some uft16LE and BE and base64 tests

type WriteSequence = [number, number][];

/**
 * Returns all possible ways to split a buffer of given length into sequential writes
 * 
 * `writeSequences(3)` will return:
 * 
 * ```
 * [
 *   [ [ 0, 3 ] ],
 *   [ [ 0, 2 ], [ 2, 3 ] ],
 *   [ [ 0, 1 ], [ 1, 3 ] ],
 *   [ [ 0, 1 ], [ 1, 2 ], [ 2, 3 ] ]
 * ]
 * ```
 */
const writeSequences = (length: number, start = 0, sequence: WriteSequence = []): WriteSequence[] => {
  if (start === length) return [sequence];

  return [...Array(length - start)].flatMap((_, i) => {
    const end = length - i;
    return writeSequences(length, end, [...sequence, [start, end]]);
  });
}

/**
 * Test StringDecoder with all possible write sequences
 */
const test = (decoder: StringDecoder, input: Buffer, expected: string) =>
  writeSequences(input.length).forEach(sequence => {
    const output = sequence.reduce(
      (result, [start, end]) => result + decoder.write(input.subarray(start, end)),
      ''
    ) + decoder.end();

    expect(output).toEqual(expected);
  });
