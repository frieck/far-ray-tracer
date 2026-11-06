#ifndef I_RAYTRACER_H
#define I_RAYTRACER_H

#include "Operacoes.h"
#include "Twister.h"
#include "PixelData.h"

namespace Raytracer {

// -----------------------------------------------------------
// Classe do Raio
// -----------------------------------------------------------
class Raio
{
public:
	Raio() : m_Origem( vetor3( 0, 0, 0 ) ), m_Direcao( vetor3( 0, 0, 0 ) ) {};
	Raio( const vetor3& a_Origem, const vetor3& a_Direcao );
	void SetOrigem( const vetor3& a_Origem ) { m_Origem = a_Origem; }
	void SetDirecao( const vetor3& a_Direcao ) { m_Direcao = a_Direcao; }
	vetor3& GetOrigem() { return m_Origem; }
	vetor3& GetDirecao() { return m_Direcao; }
private:
	vetor3 m_Origem;
	vetor3 m_Direcao;
};

// -----------------------------------------------------------
// Classe da Pilha da KD-Tree
// -----------------------------------------------------------

class NoKdTree;
struct pilhaKDTree
{
	NoKdTree* node;
	float t;
	vetor3 pb;
	int prev, dummy1, dummy2;
};

// -----------------------------------------------------------
// Classe da Engine Núcleo do RT
// -----------------------------------------------------------
class Cena;
class Primitiva;
class Esfera;
class Engine
{
public:
	Engine(Cena *c);
	~Engine();
	void SetAlvo( PixelData* a_Destino, int a_Largura, int a_Comprimento );
	Cena* GetCena() { return m_Cena; }
	Primitiva* Raytrace( Raio a_Raio, Cor& a_Cor, int a_Profundidade, float a_RIndex, float& a_Distancia );
	Primitiva* RaytraceK( Raio a_Raio, Cor& a_Cor, int a_Profundidade, float a_RIndex, float& a_Distancia, float a_Samples, float a_SScale);
	void InitRender( vetor3& a_Pos, vetor3& a_Alvo );
	bool Render();
	int retornaProgresso();
	int EncontraMaisProximo( Raio a_Raio, float& a_Dist, Primitiva*& a_Prim );
	int EncontraOclusor( Raio a_Raio, float& a_Dist );
	float CalculaShade( Esfera* a_Light, vetor3 a_IP, vetor3& a_Dir, float a_Samples, float a_SScale );
	void RenderizaComKdTree(bool a_ComKdTree){m_ComKdTree = a_ComKdTree;};
	void SetMaxProfRaio(int a_MaxProfRaio){ m_MaxProfRaio = a_MaxProfRaio;};
	Primitiva* RenderizaRaio( vetor3 a_Pos, Cor& a_Acc );
	int CalculaTamanhoTile(int a_Size);
protected:
	Cena* m_Cena;
	PixelData* m_Destino;
	int m_Largura, m_Altura, m_CurrLine, m_PPos;
	Primitiva* m_LastRow;
	float m_SScale;
	Twister m_Twister;
	int y;
	int* m_Mod;
	pilhaKDTree* m_Stack;
	bool m_ComKdTree;
	// renderer data
	vetor3 m_Origem, m_P1, m_P2, m_P3, m_P4, m_DX, m_DY;
	// tile renderer data
	int m_CurrCol, m_CurrRow;
	int m_Tiles, m_XTiles, m_YTiles;
	int m_TilesX, m_TilesY;
	int dest;
	int m_MaxProfRaio;
	bool m_OkTilesY;
public:
	// counters
	int m_RaysCast, m_Intersectaions;
};

};

#endif