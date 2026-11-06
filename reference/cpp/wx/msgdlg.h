#pragma once
#include "wx/string.h"
#include <cstdio>
inline void wxMessageBox(const wxString& s){ fprintf(stderr,"[msg] %s\n",s.c_str()); }
