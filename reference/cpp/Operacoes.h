#ifndef I_OPERACOES_H
#define I_OPERACOES_H

#include "math.h"
#include "stdlib.h"

namespace Raytracer {

inline float Rand( float a_Range ) { return ((float)rand() / RAND_MAX) * a_Range; }

#define LARGURA 800
#define ALTURA 600

// Definições matemáticas:
// Constante PI
#define PI					3.141592653589793238462f
// Constante EPSELON
#define EPSELON				0.0001f

#define SAMPLES				128

#define MAXTREEDEPTH	20

#define TILESIZE 32

// Operações:
// Produto Escalar
#define ESCALAR(A,B)		(A.x*B.x+A.y*B.y+A.z*B.z)
// Normalização do vetor
#define NORMALIZA(A)		{float l=1/sqrtf(A.x*A.x+A.y*A.y+A.z*A.z);A.x*=l;A.y*=l;A.z*=l;}
// Comprimento de um vetor
#define COMPRIMENTO(A)		(sqrtf(A.x*A.x+A.y*A.y+A.z*A.z))
// SQRComprimento
#define SQRCOMPRIMENTO(A)	(A.x*A.x+A.y*A.y+A.z*A.z)
// SQRDistancia
#define SQRDISTANCIA(A,B)	((A.x-B.x)*(A.x-B.x)+(A.y-B.y)*(A.y-B.y)+(A.z-B.z)*(A.z-B.z))

// -----------------------------------------------------------
// Classe de definição do vetor3
// -----------------------------------------------------------

class vetor3
{
public:
	///Funções 
	vetor3() : x( 0.0f ), y( 0.0f ), z( 0.0f ) {};
	vetor3( float a_X, float a_Y, float a_Z ) : x( a_X ), y( a_Y ), z( a_Z ) {};
	void Seta( float a_X, float a_Y, float a_Z ) { x = a_X; y = a_Y; z = a_Z; }
	void Normaliza() { float l = 1.0f / Comprimento(); x *= l; y *= l; z *= l; }
	float Comprimento() { return (float)sqrt( x * x + y * y + z * z ); }
	float SqrComprimento() { return x * x + y * y + z * z; }
	float Escalar( vetor3 a_V ) { return x * a_V.x + y * a_V.y + z * a_V.z; }
	float Ponto( vetor3 a_V ) { return x * a_V.x + y * a_V.y + z * a_V.z; }
	vetor3 Cross( vetor3 b ) { return vetor3( y * b.z - z * b.y, z * b.x - x * b.z, x * b.y - y * b.x ); }
	// Sobrecarga de operadores...
	void operator += ( const vetor3& a_V ) { x += a_V.x; y += a_V.y; z += a_V.z; }
	void operator += ( vetor3* a_V ) { x += a_V->x; y += a_V->y; z += a_V->z; }
	void operator -= ( const vetor3& a_V ) { x -= a_V.x; y -= a_V.y; z -= a_V.z; }
	void operator -= ( vetor3* a_V ) { x -= a_V->x; y -= a_V->y; z -= a_V->z; }
	void operator *= ( float f ) { x *= f; y *= f; z *= f; }
	void operator *= ( const vetor3& a_V ) { x *= a_V.x; y *= a_V.y; z *= a_V.z; }
	void operator *= ( vetor3* a_V ) { x *= a_V->x; y *= a_V->y; z *= a_V->z; }

	void operator /= ( float f ) { x /= f; y /= f; z /= f; }
	void operator /= ( const vetor3& a_V ) { x /= a_V.x; y /= a_V.y; z /= a_V.z; }
	void operator /= ( vetor3* a_V ) { x /= a_V->x; y /= a_V->y; z /= a_V->z; }
	vetor3 operator- () const { return vetor3( -x, -y, -z ); }
	// classes "amigas"
	friend vetor3 operator + ( const vetor3& v1, const vetor3& v2 ) { return vetor3( v1.x + v2.x, v1.y + v2.y, v1.z + v2.z ); }
	friend vetor3 operator - ( const vetor3& v1, const vetor3& v2 ) { return vetor3( v1.x - v2.x, v1.y - v2.y, v1.z - v2.z ); }
	friend vetor3 operator + ( const vetor3& v1, vetor3* v2 ) { return vetor3( v1.x + v2->x, v1.y + v2->y, v1.z + v2->z ); }
	friend vetor3 operator + ( const vetor3& v1, float v2 ) { return vetor3( v1.x + v2, v1.y + v2, v1.z + v2 ); }
	friend vetor3 operator - ( const vetor3& v1, vetor3* v2 ) { return vetor3( v1.x - v2->x, v1.y - v2->y, v1.z - v2->z ); }
	friend vetor3 operator * ( const vetor3& v, float f ) { return vetor3( v.x * f, v.y * f, v.z * f ); }
	friend vetor3 operator * ( const vetor3& v1, const vetor3& v2 ) { return vetor3( v1.x * v2.x, v1.y * v2.y, v1.z * v2.z ); }
	friend vetor3 operator * ( float f, const vetor3& v ) { return vetor3( v.x * f, v.y * f, v.z * f ); }

	friend vetor3 operator / ( const vetor3& v, float f ) { return vetor3( v.x / f, v.y / f, v.z / f ); }
	friend vetor3 operator / ( const vetor3& v1, const vetor3& v2 ) { return vetor3( v1.x / v2.x, v1.y / v2.y, v1.z / v2.z ); }
	friend vetor3 operator / ( float f, const vetor3& v ) { return vetor3( v.x / f, v.y / f, v.z / f ); }

	union
	{
		struct { float x, y, z; };
		struct { float r, g, b; };
		struct { float cell[3]; };
	};
};

// -----------------------------------------------------------
// Classe de definição da matriz
// -----------------------------------------------------------

class matriz
{
public:
	enum 
	{ 
		TX=3, 
		TY=7, 
		TZ=11, 
		D0=0, D1=5, D2=10, D3=15, 
		SX=D0, SY=D1, SZ=D2, 
		W=D3 
	};
	matriz() 
	{ 
		Identidade(); 
	}
	
	void Identidade()
	{
		cell[1] = cell[2] = cell[TX] = cell[4] = cell[6] = cell[TY] =
		cell[8] = cell[9] = cell[TZ] = cell[12] = cell[13] = cell[14] = 0;
		cell[D0] = cell[D1] = cell[D2] = cell[W] = 1;
	}

	void Rotaciona( vetor3 a_Pos, float a_RX, float a_RY, float a_RZ )
	{
		matriz t;
		t.RotacionaX( a_RZ );
		RotacionaY( a_RY );
		Concatena( t );
		t.RotacionaZ( a_RX );
		Concatena( t );
		Translada( a_Pos );
	}
	void RotacionaX( float a_RX )
	{
		float sx = (float)sinf( a_RX * PI / 180 );
		float cx = (float)cosf( a_RX * PI / 180 );
		Identidade();
		cell[5] = cx, cell[6] = sx, cell[9] = -sx, cell[10] = cx;
	}
	void RotacionaY( float a_RY )
	{
		float sy = (float)sinf( a_RY * PI / 180 );
		float cy = (float)cosf( a_RY * PI / 180 );
		Identidade ();
		cell[0] = cy, cell[2] = -sy, cell[8] = sy, cell[10] = cy;
	}
	void RotacionaZ( float a_RZ )
	{
		float sz = (float)sinf( a_RZ * PI / 180 );
		float cz = (float)cosf( a_RZ * PI / 180 );
		Identidade ();
		cell[0] = cz, cell[1] = sz, cell[4] = -sz, cell[5] = cz;
	}
	void Translada( vetor3 a_Pos )
	{ 
		cell[TX] += a_Pos.x; cell[TY] += a_Pos.y; cell[TZ] += a_Pos.z; 
	}
	void Concatena( matriz& m2 )
	{
		matriz res;
		for ( int c = 0; c < 4; c++ )
			for ( int r = 0; r < 4; r++ )
				res.cell[r * 4 + c] = cell[r * 4] * m2.cell[c] +
				  				  cell[r * 4 + 1] * m2.cell[c + 4] +
								  cell[r * 4 + 2] * m2.cell[c + 8] +
								  cell[r * 4 + 3] * m2.cell[c + 12];
		
		for ( int c = 0; c < 16; c++ ) 
			cell[c] = res.cell[c];
	}

	vetor3 Transforma( const vetor3& v )
	{
		float x  = cell[0] * v.x + cell[1] * v.y + cell[2] * v.z + cell[3];
		float y  = cell[4] * v.x + cell[5] * v.y + cell[6] * v.z + cell[7];
		float z  = cell[8] * v.x + cell[9] * v.y + cell[10] * v.z + cell[11];
		return vetor3( x, y, z );
	}

	void Inverte()
	{
		matriz t;
		float tx = -cell[3], ty = -cell[7], tz = -cell[11];
		for ( int h = 0; h < 3; h++ ) 
			for ( int v = 0; v < 3; v++ ) 
				t.cell[h + v * 4] = cell[v + h * 4];
		
		for ( int i = 0; i < 11; i++ ) 
			cell[i] = t.cell[i];
		cell[3] = tx * cell[0] + ty * cell[1] + tz * cell[2];
		cell[7] = tx * cell[4] + ty * cell[5] + tz * cell[6];
		cell[11] = tx * cell[8] + ty * cell[9] + tz * cell[10];
	}

	float cell[16];
};

// -----------------------------------------------------------
// Classe de definição do plano
// -----------------------------------------------------------

class plano
{
public:
	plano() : N( 0, 0, 0 ), D( 0 ) {};
	plano( vetor3 a_Normal, float a_D ) : N( a_Normal ), D( a_D ) {};
	union
	{
		struct
		{
			vetor3 N;
			float D;
		};
		float cell[4];
	};
};

// -----------------------------------------------------------
// Classe de definição do Box AABB
// -----------------------------------------------------------

class aabb
{
public:
	aabb() : m_Pos( vetor3( 0, 0, 0 ) ), m_Size( vetor3( 0, 0, 0 ) ) {};
	aabb( const vetor3& a_Pos, const vetor3& a_Size ) : m_Pos( a_Pos ), m_Size( a_Size ) {};
	vetor3& GetPos() { return m_Pos; }
	vetor3& GetSize() { return m_Size; }
	bool Intersecta( aabb& b2 )
	{
		vetor3 v1 = b2.GetPos(), v2 = b2.GetPos() + b2.GetSize();
		vetor3 v3 = m_Pos, v4 = m_Pos + m_Size;
		return ((v4.x >= v1.x) && (v3.x <= v2.x) && // x-axis overlap
				(v4.y >= v1.y) && (v3.y <= v2.y) && // y-axis overlap
				(v4.z >= v1.z) && (v3.z <= v2.z));   // z-axis overlap
	}
	bool Contem( vetor3 a_Pos )
	{
		vetor3 v1 = m_Pos, v2 = m_Pos + m_Size;
		return ((a_Pos.x >= v1.x) && (a_Pos.x <= v2.x) &&
				(a_Pos.y >= v1.y) && (a_Pos.y <= v2.y) &&
				(a_Pos.z >= v1.z) && (a_Pos.z <= v2.z));
	}
	float w() { return m_Size.x; }
	float h() { return m_Size.y; }
	float d() { return m_Size.z; }
	float x() { return m_Pos.x; }
	float y() { return m_Pos.y; }
	float z() { return m_Pos.z; }
private:
	vetor3 m_Pos, m_Size;
};

// Dados e variáveis específicas
// Pixel
typedef unsigned int Pixel;
typedef vetor3 Cor;

};

#endif