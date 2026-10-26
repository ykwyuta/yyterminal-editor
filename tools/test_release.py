"""Release binaries must match both architecture labels and executable formats."""
from pathlib import Path
import struct
import tempfile
import unittest

from release import dynamic_libraries, verify_binary


def elf_needing(library):
    image = bytearray(240)
    image[:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<H", image, 18, 62)
    struct.pack_into("<QHH", image, 0x20, 64, 0, 0)
    struct.pack_into("<HH", image, 0x36, 56, 2)
    struct.pack_into("<IIQQQQ", image, 64, 1, 5, 0, 0, 0, 240)  # PT_LOAD of the whole file
    struct.pack_into("<IIQQQQ", image, 120, 2, 6, 176, 176, 0, 48)  # PT_DYNAMIC
    struct.pack_into("<qQqQqQ", image, 176, 5, 239, 1, 1, 0, 0)  # DT_STRTAB, DT_NEEDED, DT_NULL
    return bytes(image) + library.encode() + b"\0"


def pe_importing(library):
    image = bytearray(0x600)
    image[:2] = b"MZ"
    struct.pack_into("<I", image, 0x3c, 64)
    image[64:68] = b"PE\0\0"
    struct.pack_into("<HH", image, 68, 0x8664, 1)
    struct.pack_into("<H", image, 84, 240)
    struct.pack_into("<H", image, 88, 0x20B)
    struct.pack_into("<I", image, 88 + 108, 16)
    struct.pack_into("<II", image, 88 + 120, 0x1000, 40)  # import directory
    struct.pack_into("<IIII", image, 88 + 240 + 8, 0x200, 0x1000, 0x200, 0x400)
    struct.pack_into("<I", image, 0x400 + 12, 0x1100)
    image[0x500:0x500 + len(library)] = library.encode()
    return bytes(image)


class ArchitectureTests(unittest.TestCase):
    def test_pe_machine_types(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test.exe"
            for architecture, machine in (("x86_64", 0x8664), ("arm64", 0xAA64)):
                image = bytearray(70)
                image[:2] = b"MZ"
                struct.pack_into("<I", image, 0x3c, 64)
                image[64:68] = b"PE\0\0"
                struct.pack_into("<H", image, 68, machine)
                path.write_bytes(image)
                verify_binary(path, "windows", architecture)
                with self.assertRaises(ValueError):
                    verify_binary(path, "windows", "arm64" if architecture == "x86_64" else "x86_64")
                with self.assertRaises(ValueError):
                    verify_binary(path, "linux", architecture)

    def test_elf_machine_types_and_invalid_files(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "test"
            for architecture, machine in (("x86_64", 62), ("arm64", 183)):
                image = bytearray(64)
                image[:6] = b"\x7fELF\x02\x01"
                struct.pack_into("<H", image, 18, machine)
                path.write_bytes(image)
                verify_binary(path, "linux", architecture)
                with self.assertRaises(ValueError):
                    verify_binary(path, "linux", "arm64" if architecture == "x86_64" else "x86_64")
                with self.assertRaises(ValueError):
                    verify_binary(path, "windows", architecture)
            path.write_bytes(b"not an executable")
            for operating_system in ("windows", "linux"):
                with self.assertRaises(ValueError):
                    verify_binary(path, operating_system, "arm64")


class DynamicLibraryTests(unittest.TestCase):
    def test_odbc_driver_manager_links_are_rejected(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "binary"
            path.write_bytes(elf_needing("libc.so.6"))
            self.assertEqual(dynamic_libraries(path, "linux"), ["libc.so.6"])
            verify_binary(path, "linux", "x86_64")
            path.write_bytes(elf_needing("libodbc.so.2"))
            with self.assertRaises(ValueError):
                verify_binary(path, "linux", "x86_64")
            path.write_bytes(pe_importing("KERNEL32.dll"))
            self.assertEqual(dynamic_libraries(path, "windows"), ["KERNEL32.dll"])
            verify_binary(path, "windows", "x86_64")
            path.write_bytes(pe_importing("ODBC32.dll"))
            with self.assertRaises(ValueError):
                verify_binary(path, "windows", "x86_64")

    def test_embedded_text_is_not_a_link(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "binary"
            path.write_bytes(elf_needing("libc.so.6") + b"no libodbc.so.2 is required\0")
            verify_binary(path, "linux", "x86_64")


if __name__ == "__main__":
    unittest.main()
