# Third-party notices

The MIT license in `LICENSE` covers the work of FARSystems. It does not change the rights
of the third parties listed here.

## Code

| Part | Origin |
| --- | --- |
| `crates/far-core/src/twister.rs` and `reference/cpp/Twister.cpp` | Mersenne Twister algorithm by Makoto Matsumoto and Takuji Nishimura |
| Triangle and box test in `kdtree.rs` | Algorithm by Tomas Akenine-Moller |

## Assets

The origin of the meshes and textures in `assets/scene-data` is not documented, so their
license has not been confirmed. They are not covered by `LICENSE`.

## Dependencies

Rust and npm dependencies keep their own licenses. See `Cargo.lock` and `package-lock.json`.

## nlohmann/json

The C++ reference uses nlohmann/json 3.12.0, vendored in
`reference/cpp/vendor/json.hpp`. Copyright (c) 2013-2025 Niels Lohmann.
Distributed under the MIT license in `reference/cpp/vendor/LICENSE.json`.
Source: https://github.com/nlohmann/json/tree/v3.12.0
