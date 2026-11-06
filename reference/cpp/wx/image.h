#pragma once
#include "wx/string.h"
#include <vector>
#include <cstdlib>
enum { wxBITMAP_TYPE_BMP, wxBITMAP_TYPE_JPEG, wxBITMAP_TYPE_PNG, wxBITMAP_TYPE_GIF, wxBITMAP_TYPE_TGA, wxBITMAP_TYPE_ANY };
class wxImage { std::vector<unsigned char> d; int w=1,h=1;
public:
 wxImage(){ d.assign(3,128);}  
 bool LoadFile(const char* p,int=wxBITMAP_TYPE_ANY){
  std::string t=std::string(getenv("FAR_TMP"))+"/t.bmp";
  std::string c="sips -s format bmp -s formatOptions default \""+std::string(p)+"\" --out \""+t+"\" >/dev/null 2>&1";
  if(system(c.c_str())!=0){fprintf(stderr,"[image] failed: %s\n",p);return false;}
  FILE*f=fopen(t.c_str(),"rb"); if(!f)return false; std::vector<unsigned char> b; int ch; while((ch=fgetc(f))!=EOF)b.push_back(ch); fclose(f);
  unsigned off=*(unsigned*)&b[10]; int W=*(int*)&b[18],H=*(int*)&b[22]; int bpp=*(unsigned short*)&b[28]; bool td=H<0; if(td)H=-H; int bp=bpp/8; int st=((W*bp+3)/4)*4;
  w=W;h=H;d.resize(W*H*3); for(int y=0;y<H;y++){int sy=td?y:H-1-y; for(int x=0;x<W;x++){unsigned char*s=&b[off+sy*st+x*bp]; d[(y*W+x)*3]=s[2];d[(y*W+x)*3+1]=s[1];d[(y*W+x)*3+2]=s[0];}}
  return true; }
 int GetWidth(){return w;} int GetHeight(){return h;} unsigned char* GetData(){return d.data();} bool IsOk(){return true;} bool Ok(){return true;} void Destroy(){} };
