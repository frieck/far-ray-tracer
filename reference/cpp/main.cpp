#include <vector>
#include "Cena.h"
#include "RayTracer.h"
#include "PixelData.h"
#include <chrono>
using namespace Raytracer;
std::string g_exe_s; const char* g_exe;
int main(int c,char**v){ // usage: farrt <assets> <scene.frt> <output.bmp> <kd> <width> <height>
 g_exe_s=std::string(v[1])+"\\x.exe"; g_exe=g_exe_s.c_str(); int kd=atoi(v[4]),W=atoi(v[5]),H=atoi(v[6]);
 Cena* cena=new Cena(); PixelData px(W,H); Engine* rt=new Engine(cena);
 cena->AbreCena(v[2]); cena->PreparaCena();
 auto t0=std::chrono::steady_clock::now();
 if(kd) cena->ConstroiKdTree();
 auto t1=std::chrono::steady_clock::now();
 cena->setAmbCor(Cor(0,0,0)); rt->RenderizaComKdTree(kd); if(getenv("DEPTH")) rt->SetMaxProfRaio(atoi(getenv("DEPTH"))); rt->SetAlvo(&px,W,H);
 vetor3 a(0,0,-2),b(0,0,5); rt->InitRender(a,b); while(!rt->Render()); 
 auto t2=std::chrono::steady_clock::now();
 auto ms=[](auto a,auto b){return (long)std::chrono::duration_cast<std::chrono::milliseconds>(b-a).count();};
 printf("primitives=%d lights=%d kd=%dms render=%dms rays=%d\n",cena->GetNrPrimitivas(),cena->GetNrLuzes(),(int)ms(t0,t1),(int)ms(t1,t2),rt->m_RaysCast);
 FILE*f=fopen(v[3],"wb"); int st=((W*3+3)/4)*4; unsigned sz=54+st*H; unsigned char h[54]={'B','M'}; *(unsigned*)(h+2)=sz; *(unsigned*)(h+10)=54; *(unsigned*)(h+14)=40; *(int*)(h+18)=W; *(int*)(h+22)=H; *(unsigned short*)(h+26)=1; *(unsigned short*)(h+28)=24; fwrite(h,1,54,f);
 float* d=px.RetornaPixelData(); std::vector<unsigned char> row(st);
 for(int y=0;y<H;y++){ for(int x=0;x<W;x++){ float*p=d+(y*W+x)*3; for(int k=0;k<3;k++){float q=p[2-k]*255.f+.5f; row[x*3+k]=q>255?255:q<0?0:(unsigned char)q;} } fwrite(row.data(),1,st,f);} fclose(f);
}
