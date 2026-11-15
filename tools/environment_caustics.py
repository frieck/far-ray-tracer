"""Generate a CPU example lit only by the physical sky, with reflective and refractive caustics."""
import json
from pathlib import Path

scene = {
    "format": 3,
    "camera": {"position": [0, 4, -3], "target": [0, -0.3, 6], "zoom": 1.3},
    "render": {"app": 1, "quality": 256, "toneMapping": "agx_punchy", "denoise": True,
               "caustics": True, "causticPreset": "balanced", "photonMillions": 6, "causticRadius": 0.12},
    "environment": {"sky": {"elevation": 35, "azimuth": 35, "turbidity": 2,
                            "sun_size": 1, "sun_strength": 1}, "intensity": 1, "visible": True},
    "lights": [],
    "objects": [
        {"type": "ground", "position": [0, -1.5, 0], "normal": [0, 1, 0],
         "material": {"color": [0.7, 0.7, 0.7], "diffuse": 1, "specular": 0}},
        {"type": "sphere", "center": [-2, 0, 6], "radius": 1.5,
         "material": {"color": [1, 1, 1], "diffuse": 0, "specular": 0, "refraction": 1, "ior": 1.5}},
        {"type": "sphere", "center": [2, 0, 6], "radius": 1.5,
         "material": {"color": [0.9, 0.65, 0.3], "surface": {"preset": "metal", "roughness": 0.05}}},
    ],
}
path = Path(__file__).resolve().parents[1] / "assets/scenes/techniques/environment-caustics.far"
path.write_text(json.dumps(scene, indent=2) + "\n")
print(path)
