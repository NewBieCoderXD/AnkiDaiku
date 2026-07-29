"""Validate a generated .apkg file using Anki's actual import code."""

import json
import os
import shutil
import sys
import tempfile
import zipfile

ANKI_BASE = os.environ.get("ANKI_BASE", "/opt/anki")
sys.path.insert(0, os.path.join(ANKI_BASE, "app_packages"))


def validate_apkg(apkg_path: str) -> bool:
    with zipfile.ZipFile(apkg_path) as z:
        names = z.namelist()

        assert "collection.anki2" in names, "Missing collection.anki2"

        if "media" in names:
            media = json.loads(z.read("media"))
            for key, val in media.items():
                int(key)
                assert "/" not in val, f"Media filename '{val}' contains '/'"
                assert "\\" not in val, f"Media filename '{val}' contains '\\'"
                assert val, f"Media filename for key {key} is empty"

        for name in names:
            if name not in ("collection.anki2", "media"):
                int(name)

    from anki.collection import Collection
    from anki.import_export_pb2 import (
        ImportAnkiPackageOptions,
        ImportAnkiPackageRequest,
    )

    tmpdir = tempfile.mkdtemp()
    col = Collection(os.path.join(tmpdir, "validation.anki2"))
    req = ImportAnkiPackageRequest(
        package_path=apkg_path, options=ImportAnkiPackageOptions()
    )
    try:
        result = col.import_anki_package(req)
        print(f"OK: {result.log.found_notes} note(s) imported")
        return True
    except Exception as e:
        print(f"FAIL: {e}")
        return False
    finally:
        col.close()
        shutil.rmtree(tmpdir)


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: test_apkg.py <path/to/file.apkg>")
        sys.exit(1)
    success = validate_apkg(sys.argv[1])
    sys.exit(0 if success else 1)
