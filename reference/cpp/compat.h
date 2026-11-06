#pragma once
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <chrono>
#include <string>
inline std::string fix_utf8(const char* p){std::string o;const unsigned char* s=(const unsigned char*)p;while(*s){unsigned c=*s;int n=c<0x80?0:(c>>5)==6?1:(c>>4)==14?2:(c>>3)==30?3:-1;bool ok=n>=0;for(int i=1;ok&&i<=n;i++)ok=(s[i]&0xC0)==0x80;if(ok){o.append((const char*)s,n+1);s+=n+1;}else{o+=(char)(0xC0|(c>>6));o+=(char)(0x80|(c&63));s++;}}return o;}
inline int fopen_s(FILE** f,const char* n,const char* m){*f=fopen(fix_utf8(n).c_str(),m);return *f?0:1;}
inline unsigned GetTickCount(){using namespace std::chrono;return (unsigned)duration_cast<milliseconds>(steady_clock::now().time_since_epoch()).count();}
