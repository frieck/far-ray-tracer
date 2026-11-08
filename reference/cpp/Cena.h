#ifndef __CENA_H__
#define __CENA_H__

#include <wx/string.h>
#include <wx/image.h>
#include <wx/bitmap.h>
#include "raytracer.h"
#include "ldde.h"

namespace Raytracer {

// Valores do teste de intersecção
#define HIT		 1		// O raio colide com a primitiva
#define MISS	 0		// O raio não colide
#define INPRIM	-1		// O raio está na primitiva


// -----------------------------------------------------------
// Classe de definição de texturas
// -----------------------------------------------------------

class Textura
{
public:
	//Textura( wxImage *a_Imagem );
	Textura( wxString a_Arquivo );
	Cor* GetImagem() { return m_Imagem; }
	Cor GetTexel( float a_U, float a_V );
	int GetLargura() { return m_Largura; }
	int GetAltura() { return m_Altura; }
	wxString GetFile(){return m_File;};
private:
	Cor *m_Imagem;
	wxString m_File;
	int m_Altura;
	int m_Largura;
	int Modulo(int x);
};

// -----------------------------------------------------------
// Classe de definição de materiais
// -----------------------------------------------------------

class Material
{
public:
	Material();
	void SetCor( Cor& a_Cor ) { m_Cor = a_Cor; }
	Cor GetCor() { return m_Cor; }
	void SetDiffuse( float a_Diff ) { m_Diff = a_Diff; }
	void SetSpecular( float a_Spec ) { m_Spec = a_Spec; }
	void SetReflexao( float a_Refl ) { m_Refl = a_Refl; }
	void SetRefracao( float a_Refr ) { m_Refr = a_Refr; }
	void SetRefrIndex( float a_Refr ) { m_RIndex = a_Refr; }
	void SetDiffuseRefl( float a_DRefl ) { m_DRefl = a_DRefl; }

	float GetSpecular() { return m_Spec; }
	float GetDiffuse() { return m_Diff; }
	float GetReflexao() { return m_Refl; }
	float GetRefracao() { return m_Refr; }	
	float GetRefrIndex() { return m_RIndex; }	
	float GetDiffuseRefl() { return m_DRefl; }

	void SetTextura( Textura* a_Textura ) { m_Textura = a_Textura; }
	Textura* GetTextura() { return m_Textura; }
	void SetaEscalaUV( float a_UScale, float a_VScale );
	float GetUScale() { return m_UScale; }
	float GetVScale() { return m_VScale; }
	float GetUScaleReci() { return m_RUScale; }
	float GetVScaleReci() { return m_RVScale; }

private:
	Cor m_Cor;
	float m_Refl;
	float m_Diff;
	float m_Spec;
	float m_Refr;
	float m_RIndex;
	float m_DRefl;

	Textura* m_Textura;

	float m_UScale;
	float m_VScale;
	float m_RUScale;
	float m_RVScale;
};

// -----------------------------------------------------------
// Classe de definição de vértices
// -----------------------------------------------------------

class Vertice
{
public:
	Vertice() {};
	Vertice( vetor3 a_Pos, float a_U, float a_V );
	float GetU() { return m_U; }
	float GetV() { return m_V; }
	vetor3& GetNormal() { return m_Normal; }
	vetor3& GetPos() { return m_Pos; }
	void SetUV( float a_U, float a_V ) { m_U = a_U; m_V = a_V; }
	void SetPos( vetor3& a_Pos ) { m_Pos = a_Pos; }
	void SetNormal( vetor3& a_N ) { m_Normal = a_N; }
private:
	vetor3 m_Pos;
	vetor3 m_Normal;
	float m_U;
	float m_V;
};

// -----------------------------------------------------------
// Classe de definição de primitivas (herdada pelas outras primitivas)
// -----------------------------------------------------------

class Primitiva
{
public:
	enum
	{
		ESFERA = 1, PLANO, QUAD, TRIANGULO, MALHA, ABB, LUZ, CILINDRO
	};
	Primitiva() : m_Luz( false ), m_Tipo(0) { m_Material = new Material();};
	Material* GetMaterial() { return m_Material; }
	void SetMaterial( Material *a_Mat ) { m_Material = a_Mat; }
	int GetTipo(){return m_Tipo;};
	virtual int Intersepta( Raio& a_Raio, float& a_Distancia ){return 0;};
	virtual vetor3 GetNormal( vetor3& a_Posicao ){return vetor3(0,0,0);};
	virtual bool IntersectaBox( aabb& a_Box );
	vetor3 GetNormal(){return m_N;};
	virtual Cor GetCor( vetor3& ) { return m_Material->GetCor(); }
	virtual void Luz( bool a_Luz ) { m_Luz = a_Luz; }
	virtual void SetNome(wxString n){m_Name = n;}
	virtual void CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis ){};
	wxString GetNome(){return m_Name;}
	bool ELuz() { return m_Luz; }
	bool SobrepoemPlanoBox( vetor3& a_Normal, vetor3& a_Vert, vetor3& a_MaxBox );
protected:
	Material* m_Material;
	bool m_Luz;
	wxString m_Name;
	vetor3 m_N;
	int m_Tipo;
};

// -----------------------------------------------------------
// Classe de definição de esferas
// -----------------------------------------------------------

class Esfera : public Primitiva
{
public:
	Esfera( vetor3& a_Centro, float a_Raio );
	Esfera();
	vetor3& GetCentro() { return m_Centro; }
	float GetSqRaio() { return m_SqRaio; }
	float GetRaio() { return m_Raio; }
	Cor GetCor( vetor3& a_Pos );
	int Intersepta( Raio& a_Raio, float& a_Distancia );
	vetor3 GetNormal( vetor3& a_Pos ) { return (a_Pos - m_Centro) * m_RRaio; }
	void CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis );
	void SetCentro(vetor3 a_Centro){ m_Centro = a_Centro; }
	void SetRaio(float a_Raio){ m_Raio = a_Raio; m_SqRaio = a_Raio * a_Raio; m_RRaio = 1.0f / a_Raio;}
	virtual bool IntersectaEsferaBox( aabb& a_Box );
private:
	vetor3 m_Centro;
	float m_SqRaio, m_Raio, m_RRaio;
	vetor3 m_Ve, m_Vn, m_Vc;
};

// -----------------------------------------------------------
// Classe de definição de cilindros
// NÃO FUNCIONA CORRETAMENTE!
// -----------------------------------------------------------

class Cilindro : public Primitiva
{
public:
	Cilindro( float a_Raio, float a_Altura, vetor3 a_Pos, float a_AngX, float a_AngY, float a_AngZ );
	Cilindro();
	float GetRaio() { return m_Raio; }
	float GetAltura() { return m_Altura; }
	vetor3 GetPos() { return m_Pos; }
	float GetAngX() { return m_AngX; }
	float GetAngY() { return m_AngY; }
	float GetAngZ() { return m_AngZ; }

	int InterseptaPlano( plano p, Raio& a_Raio, float& a_Dist );

	Cor GetCor( vetor3& a_Pos );

	int Intersepta( Raio& a_Raio, float& a_Distancia );
	vetor3 GetNormal( vetor3& a_Pos );
	void CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis );
	
	void SetRaio( float a_Raio ){ m_Raio = a_Raio; }
	void SetAltura( float a_Altura ){ m_Altura = a_Altura; }
	void SetPos( vetor3 a_Pos );
	void SetAng( float a_AngX, float a_AngY, float a_AngZ );
	
	virtual bool IntersectaCilindroBox( aabb& a_Box );
private:
	float m_Raio;
	float m_Altura;
	matriz m_Matriz;
	vetor3 m_Pos;
	float m_AngX;
	float m_AngY;
	float m_AngZ;
};

// -----------------------------------------------------------
// Classe de definição de Box
// -----------------------------------------------------------

class Box : public Primitiva
{
public:
	int GetTipo() { return QUAD; }
	Box() : m_Pos( vetor3( 0, 0, 0 )) , m_Size( vetor3( 0, 0, 0 )) {};
	Box( vetor3 a_Pos, vetor3 a_Size );
	int Intersepta( Raio& a_Raio, float& a_Distancia );	
	vetor3 GetNormal( vetor3& );	
	
protected:
	vetor3 m_Pos, m_Size;
};

// -----------------------------------------------------------
// Classe de definição de triângulos
// -----------------------------------------------------------

class Triangulo : public Primitiva
{
public:
	Triangulo();
	Triangulo(Vertice* a_V1, Vertice* a_V2, Vertice* a_V3 );
	~Triangulo();
	Cor GetCor( vetor3& a_Pos );
	int Intersepta( Raio& a_Raio, float& a_Distancia );
	void CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis );
	vetor3 GetNormal( vetor3& a_Pos );
	Vertice* GetVertice( int a_Idx ) { return m_Vertex[a_Idx]; }
	virtual bool IntersectaTriBox( vetor3 a_BoxCentre, vetor3 a_BoxHalfsize );
	float GetU(){ return m_U; };
	float GetV(){ return m_V; };
	vetor3 GetN(){ return m_N; };
	float GetNU(){ return nu; };
	float GetNV(){ return nv; };
	float GetND(){ return nd; };
	float GetBNU(){ return bnu; };
	float GetBNV(){ return bnv; };
	float GetCNU(){ return cnu; };
	float GetCNV(){ return cnv; };
	int GetK(){ return k; };
private:
	Vertice *m_Vertex[3];
	float m_U, m_V;
	float nu, nv, nd;
	float bnu, bnv;
	int k;
	float cnu, cnv;
};

// -----------------------------------------------------------
// Classe de definição de Malha
// -----------------------------------------------------------

class Malha : public Primitiva
{
public:
	Malha() : m_Cena(0), m_Size(1) {m_Triangulos = new ListaDDE(sizeof(Primitiva *)); m_Arquivo.Printf(""); m_Tipo = MALHA;};
	Malha(Cena *c, wxString a_Arquivo, Material* a_Material, vetor3& a_Pos, float a_Size );

	void SetCena(Cena *a_Cena){m_Cena = a_Cena;};
	void SetMaterial(Material *a_Material);
	void SetPos(vetor3 a_Pos);
	void SetSize(float a_Size);
	void CarregaArquivo(wxString a_Arquivo);

	Material* GetMaterial(){return m_Material;};
	vetor3 GetPos(){return m_Pos;};
	float GetSize(){return m_Size;};
	wxString GetArquivo(){return m_Arquivo;};

	~Malha();
	Cor GetCor( vetor3& a_Pos ){return Cor(0,0,0);};
	int Intersepta( Raio& a_Raio, float& a_Distancia ){return MISS;};
	vetor3 GetNormal( vetor3& a_Pos ){return vetor3(0,0,0);};
	ListaDDE *m_Triangulos;
	void RemoveMalha();
private:
	Cena *m_Cena;
	wxString m_Arquivo;
	vetor3 m_Pos;
	float m_Size;
};

// -----------------------------------------------------------
// Classe de definição de planos com base em triângulos
// -----------------------------------------------------------

class Plano : public Primitiva
{
public:
	Plano(Cena *a_Cena);;
	Plano(Cena *c, Material *a_Material, vetor3 a_P1, vetor3 a_P2, vetor3 a_P3, vetor3 a_P4);

	void SetCena(Cena *a_Cena){m_Cena = a_Cena;};
	void SetMaterial(Material *a_Material);
	void SetPlano(vetor3 a_P1, vetor3 a_P2, vetor3 a_P3, vetor3 a_P4);
	vetor3 GetPos(int a_Idx);

	~Plano();
	Cor GetCor( vetor3& a_Pos ){return Cor(0,0,0);};
	int Intersepta( Raio& a_Raio, float& a_Distancia ){return MISS;};
	vetor3 GetNormal( vetor3& a_Pos ){return vetor3(0,0,0);};

	void RemovePlano();

	ListaDDE *m_Triangulos;
private:
	Cena *m_Cena;
};

// -----------------------------------------------------------
// Lista de objetos da KD-Tree
// -----------------------------------------------------------

class ListaObjeto
{
public:
	ListaObjeto() : m_Primitiva( 0 ), m_Next( 0 ) {}
	~ListaObjeto() { delete m_Next; }
	void SetPrimitiva( Primitiva* a_Prim ) { m_Primitiva = a_Prim; }
	Primitiva* GetPrimitiva() { return m_Primitiva; }
	void SetNext( ListaObjeto* a_Next ) { m_Next = a_Next; }
	ListaObjeto* GetNext() { return m_Next; }
private:
	Primitiva* m_Primitiva;
	ListaObjeto* m_Next;
};

// -----------------------------------------------------------
// Classe de definição do Nó da KD-Tree
// -----------------------------------------------------------

class ManipuladorMemoria;
class NoKdTree
{
public:
	NoKdTree() : m_Data( 6 ) {};
	void SetEixo( int a_Axis ) { m_Data = (m_Data & ~(uintptr_t)3) + a_Axis; }
	int GetEixo() { return m_Data & 3; }
	void SetPosicaoDivisao( float a_Pos ) { m_Div = a_Pos; }
	float GetPosicaoDivisao() { return m_Div; }
	void SetEsquerda( NoKdTree* a_Left ) { m_Data = (uintptr_t)a_Left + (m_Data & 7); }
	NoKdTree* GetEsquerda() { return (NoKdTree*)(m_Data&~(uintptr_t)7); }
	NoKdTree* GetDireita() { return ((NoKdTree*)(m_Data&~(uintptr_t)7)) + 1; }
	void Adiciona( Primitiva* a_Prim );
	bool EFolha() { return ((m_Data & 4) > 0); }
	void SetFolha( bool a_Leaf ) { m_Data = (a_Leaf)?(m_Data|4):(m_Data&~(uintptr_t)4); }
	ListaObjeto* GetLista() { return (ListaObjeto*)(m_Data&~(uintptr_t)7); }
	void SetLista( ListaObjeto* a_List ) { m_Data = (uintptr_t)a_List + (m_Data & 7); }
private:
	// int m_Flags;
	float m_Div;
	uintptr_t m_Data;
};

// -----------------------------------------------------------
// Classe de definição da lista de possíveis divisões
// -----------------------------------------------------------

struct ListaDivisao
{
	float posdiv;
	int contN1, contN2;
	ListaDivisao* prox;
};

// -----------------------------------------------------------
// Classe de definição da KD-Tree
// -----------------------------------------------------------

class KdTree
{
public:
	KdTree();
	~KdTree();
	void Constroi( Cena *a_Cena );
	NoKdTree* GetRaiz() { return m_Raiz; }
	void SetRaiz( NoKdTree* a_Raiz ) { m_Raiz = a_Raiz; }
	// tree generation
	void InserePosicaoDivisao( float a_SplitPos );
	void Subdivide( NoKdTree* a_No, aabb& a_Box, int a_Prof, int a_Prims );
	// memory manager
	static void SetManipuladorMemoria( ManipuladorMemoria* a_MM ) { s_MMemoria = a_MM; }
private:
	NoKdTree* m_Raiz;
	ListaDivisao* m_SLista, *m_SPool;
public:
	static ManipuladorMemoria* s_MMemoria;
	int m_Prof;
};

// -----------------------------------------------------------
// Classe de definição de cena
// -----------------------------------------------------------

class Cena
{
public:
	Cena();
	~Cena();
	void Cena::setFog(float fog){m_Fog = fog/10;}
	float Cena::getFog(){return m_Fog;}
	void Cena::setFogCor(Cor c){m_FogCor = c;}
	void Cena::setAmbCor(Cor c){m_Ambiente = c;}
	Cor Cena::getFogCor(){return m_FogCor;}
	int Cena::quantMaterial(char * linha);
	void Cena::atribuiMaterial(char *tipo, char *material, Primitiva *prim);
	void CarregaCena(char arq[]);
	int GetNrPrimitivas() { return m_NPrimi; }
	int GetNrLuzes() { return m_NLuzes; }
	Cor getCorAmbiente(){ return m_Ambiente; }
	Primitiva* GetPrimitiva( int a_Idx );
	Primitiva* GetLuz( int a_Idx );
	aabb& GetExtends() { return m_Extends; }
	ListaDDE *m_Malhas;
	ListaDDE *m_Planos;
	ListaDDE *m_Esferas;
	ListaDDE *m_Cubos;
	void Carrega3DS( ListaDDE *m_Triangulos, wxString a_Arquivo, Material* a_Material, vetor3& a_Pos, float a_Size );
	long ComeLixo( char* buffer );
	void ConstroiKdTree();
	KdTree* GetKdTree() { return m_KdTree; }
	bool kdTreeGerada;
	void PreparaCena();
	int ContaPrim();
	int ContaLuz();
	int NumeroPrimitivas(){return m_NPrimi + m_NLuzes;};
	void AbreCena(wxString a);

	int GetTamanhoTextura(){return m_TamTextura;};
	float* RetornaInfoTextura();
	int GetTamanhoTexturaLuzes(){return m_TamLuzes;};
	float* RetornaInfoTexturaLuzes();
	void PreparaTextura();
private:
	float* m_Data;
	float* m_DataLuzes;
	int m_TamTextura;
	int m_TamLuzes;
	//int m_Primitivas;
	Cor m_Ambiente;
	float m_Fog;
	Cor m_FogCor;
	float *m_Verts;
	float *m_TCoords;
	int m_NrVerts;
	int m_NrFaces;
	unsigned short* m_Faces;
	unsigned char* temp;
	unsigned char* alinhado;
	inline float GetF( char* addr );
	inline unsigned short GetW( char* addr );
	inline unsigned long GetD( char* addr );
	Primitiva **m_Primi;
	Primitiva **m_Luzes;
	int m_NPrimi;
	int m_NLuzes;
	aabb m_Extends;
	KdTree* m_KdTree;
};

};

#endif