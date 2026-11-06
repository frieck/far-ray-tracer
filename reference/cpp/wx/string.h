#pragma once
#include <string>
#include <cstdio>
#include <cstdarg>
#include <cstring>
#include <algorithm>
class wxString : public std::string {
public:
  wxString() {}
  wxString(const char* s) : std::string(s ? s : "") {}
  wxString(const std::string& s) : std::string(s) {}
  int Printf(const char* f, ...) { char b[4096]; va_list a; va_start(a,f); int n=vsnprintf(b,sizeof b,f,a); va_end(a); assign(b); return n; }
  wxString BeforeLast(char c) const { auto p=rfind(c); return p==npos?wxString():wxString(substr(0,p)); }
  wxString AfterLast(char c) const { auto p=rfind(c); return p==npos?*this:wxString(substr(p+1)); }
  wxString Right(size_t n) const { return size()<=n?*this:wxString(substr(size()-n)); }
  wxString Lower() const { wxString r=*this; std::transform(r.begin(),r.end(),r.begin(),::tolower); return r; }
  wxString MakeLower() { *this=Lower(); return *this; }
  wxString MakeUpper() { std::transform(begin(),end(),begin(),::toupper); return *this; }
  wxString& Prepend(const wxString& o){ insert(0,o); return *this; }
  void LowerCase(){ *this=Lower(); }
  bool EndsWith(const char* e) const { size_t n=strlen(e); return size()>=n && compare(size()-n,n,e)==0; }
  size_t Len() const { return size(); }
  bool IsEmpty() const { return empty(); }
  int Find(const char* s) const { auto p=find(s); return p==npos?-1:(int)p; }
  wxString Mid(size_t a,size_t n=npos) const { return a>=size()?wxString():wxString(substr(a,n)); }
};
