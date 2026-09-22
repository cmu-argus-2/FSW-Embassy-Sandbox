#!/usr/bin/env python3
import struct
import sys

# Simplified UF2 converter for RP2350
# Usage: python3 elf2uf2.py input.bin output.uf2

UF2_MAGIC_START0 = 0x0A324655
UF2_MAGIC_START1 = 0x9E5D5157
UF2_MAGIC_END    = 0x0AB16F30
RP2350_FAMILY_ID = 0xe48bff56 # ARM (v8-M) family ID

def convert(input_path, output_path):
    with open(input_path, "rb") as f:
        data = f.read()

    blocks = []
    num_blocks = (len(data) + 255) // 256
    
    for i in range(num_blocks):
        block = bytearray(512)
        # Header
        struct.pack_into("<LLLL", block, 0, UF2_MAGIC_START0, UF2_MAGIC_START1, 0x00002000, 0x10000000 + (i * 256))
        struct.pack_into("<LLLL", block, 16, 256, i, num_blocks, RP2350_FAMILY_ID)
        
        # Data
        chunk = data[i*256 : (i+1)*256]
        block[32 : 32+len(chunk)] = chunk
        
        # Footer
        struct.pack_into("<L", block, 508, UF2_MAGIC_END)
        blocks.append(block)

    with open(output_path, "wb") as f:
        for b in blocks:
            f.write(b)
    print(f"Converted {input_path} to {output_path} ({num_blocks} blocks)")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python3 elf2uf2.py input.bin output.uf2")
    else:
        convert(sys.argv[1], sys.argv[2])
