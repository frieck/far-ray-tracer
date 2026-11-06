#include "PixelData.h"

PixelData::PixelData(int width, int height)
{
	m_PixelData = (float *)malloc(sizeof(float *) * width * height * 3);
	m_Width = width;
	m_Height = height;
}

void PixelData::Realoca(int width, int height)
{
	m_PixelData = (float *)malloc(sizeof(float *) * width * height * 3);
	m_Width = width;
	m_Height = height;
}

PixelData::~PixelData()
{
	free(m_PixelData);
}

void PixelData::SetaValor(int x, int y, wxColour c)
{
	int m_X = m_Width - x;
	int m_Y = m_Height - y;

	if((m_X >= m_Width) || (m_X < 0) || (m_Y >= m_Height) || (m_Y < 0))
		return;

	int p = m_Y * m_Width - m_X;

	p *= 3;
	if(p > (m_Width * m_Height * 3))
		return;
	m_PixelData[p] = c.Red() / 255.0f;
	m_PixelData[p+1] = c.Green() / 255.0f;
	m_PixelData[p+2] = c.Blue() / 255.0f;
}

void PixelData::SetaValor(int p, wxColour c)
{
	if((p * 3) > (m_Width * m_Height * 3))
		return;
	
	int lin = p % m_Width;
	int col = p / m_Width;

	SetaValor(lin, col, c);
	/*p *= 3;
	if(p > (m_Width * m_Height * 3))
		return;
	m_PixelData[p] = c.Red() / 255.0f;
	m_PixelData[p+1] = c.Green() / 255.0f;
	m_PixelData[p+2] = c.Blue() / 255.0f;*/
}