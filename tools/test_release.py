"""Release binaries must match both architecture labels and executable formats."""
from pathlib import Path
import struct
import tempfile
import unittest

from release import verify_binary


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


if __name__ == "__main__":
    unittest.main()
