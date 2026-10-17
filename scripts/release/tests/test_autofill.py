"""Every terminal pane is an NSTextInputClient, so macOS treats it as a field
that may take a verification code from Messages and shows a "Fill code" pill
that typing cannot dismiss. Requiring the one-time-code content type limits
that offer to fields that declare it, which no Herdr view does. macOS reads
the key from the main bundle, which every build copies from this plist."""
from pathlib import Path
import plistlib
import unittest


ROOT = Path(__file__).resolve().parents[3]


class AutoFillTests(unittest.TestCase):
    def test_bundle_plist_requires_one_time_code_content_type(self):
        info = plistlib.loads((ROOT / "assets/macos/Info.plist").read_bytes())
        self.assertIs(info.get("NSAutoFillRequiresTextContentTypeForOneTimeCodeOnMac"), True)


if __name__ == "__main__":
    unittest.main()
