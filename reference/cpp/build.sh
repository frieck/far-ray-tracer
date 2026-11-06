#!/bin/sh
# Builds the C++ reference: the original engine with its 64-bit fixes.
# -ffp-contract=off keeps ARM from fusing multiply and add, so results match Rust.
cd "$(dirname "$0")" && clang++ -std=c++17 -O1 -ffp-contract=off -fms-extensions -Wno-everything \
  -include compat.h -include wx/msgdlg.h -I. \
  Cena.cpp RayTracer.cpp memory.cpp Twister.cpp LDDE.cpp PixelData.cpp main.cpp -o farrt
