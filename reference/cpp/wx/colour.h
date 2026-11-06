#pragma once
class wxColour { unsigned char r_,g_,b_; public: wxColour(int r=0,int g=0,int b=0):r_(r),g_(g),b_(b){} unsigned char Red()const{return r_;} unsigned char Green()const{return g_;} unsigned char Blue()const{return b_;} };
