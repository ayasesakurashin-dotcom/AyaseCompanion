"""Re-encode the user-supplied square JPG as Windows icon sizes; no artwork changes."""
from pathlib import Path
from PIL import Image

root = Path(__file__).resolve().parent.parent
source = Image.open(root / "icon/icon.jpg").convert("RGBA")
if source.width != source.height:
    raise ValueError("Provide a square source image; this tool does not crop artwork.")
dest = root / "apps/desktop/src-tauri/icons"
dest.mkdir(parents=True, exist_ok=True)
icon = source.resize((256, 256), Image.Resampling.LANCZOS)
icon.save(dest / "icon.png")
icon.save(dest / "icon.ico", sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])
print("Generated icon.png and multi-size icon.ico from icon/icon.jpg")
