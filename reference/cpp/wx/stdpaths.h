#pragma once
#include "wx/string.h"
extern const char* g_exe;
class wxStandardPaths { public: wxString GetExecutablePath(){ return g_exe; } };
