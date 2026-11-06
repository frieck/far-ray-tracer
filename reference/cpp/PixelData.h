#ifndef __PIXELDATA_h__
#define __PIXELDATA_h__

#include <wx/colour.h>

class PixelData
{
	public:
		PixelData(int width, int height);
		~PixelData();
		void Realoca(int width, int height);
		float *RetornaPixelData(){return m_PixelData;};
		void SetaValor(int x, int y, wxColour c);
		void SetaValor(int p, wxColour c);
		int GetWidth(){return m_Width;};
		int GetHeight(){return m_Height;};
	private:
		float *m_PixelData;
		int m_Width;
		int m_Height;
};

#endif