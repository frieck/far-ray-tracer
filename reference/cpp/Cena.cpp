#include "Operacoes.h"
#include "Cena.h"
#include "RayTracer.h"
#include <stdio.h>
#include <wx/stdpaths.h>
#include "memory.h"
#include "Tela.h"

#define FLT_MAX 3.402823466e+38F

namespace Raytracer {

ManipuladorMemoria* KdTree::s_MMemoria = 0;

//------------------------------------------------------------
// Seta a Escala UV do Material
//------------------------------------------------------------
void Material::SetaEscalaUV( float a_UScale, float a_VScale )
{ 
	m_UScale = a_UScale; 
	m_VScale = a_VScale; 
	m_RUScale = 1.0f / a_UScale;
	m_RVScale = 1.0f / a_VScale;
}

//------------------------------------------------------------
// Calcula o intervalo do cubo que compõe o triângulo
//------------------------------------------------------------
void Triangulo::CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis )
{
	vetor3 pos1 = m_Vertex[0]->GetPos();
	a_Pos1 = pos1.cell[a_Axis], a_Pos2 = pos1.cell[a_Axis];
	for ( int i = 1; i < 3; i++ )
	{
		vetor3 pos = m_Vertex[i]->GetPos();
		if (pos.cell[a_Axis] < a_Pos1) a_Pos1 = pos.cell[a_Axis];
		if (pos.cell[a_Axis] > a_Pos2) a_Pos2 = pos.cell[a_Axis];
	}
}

//------------------------------------------------------------
// Calcula o intervalo do cubo que compõe a esfera
//------------------------------------------------------------
void Esfera::CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis )
{
	a_Pos1 = m_Centro.cell[a_Axis] - m_Raio;
	a_Pos2 = m_Centro.cell[a_Axis] + m_Raio;
}

//------------------------------------------------------------
// Calcula o intervalo do cubo que compõe o cilindro
//------------------------------------------------------------
void Cilindro::CalculaIntervalo( float& a_Pos1, float& a_Pos2, int a_Axis )
{
	if(a_Axis == 0)
	{
		a_Pos1 = -m_Raio;
		a_Pos2 = m_Raio;

		vetor3 aux1 = vetor3(a_Pos1, 0, 0);
		vetor3 aux2 = vetor3(a_Pos2, 0, 0);
		a_Pos1 = m_Matriz.Transforma(aux1).x;
		a_Pos2 = m_Matriz.Transforma(aux2).x;
	}
	else
	{
		a_Pos1 = 0;
		a_Pos2 = m_Altura;

		vetor3 aux1 = vetor3(0, a_Pos1, 0);
		vetor3 aux2 = vetor3(0, a_Pos2, 0);
		a_Pos1 = m_Matriz.Transforma(aux1).y;
		a_Pos2 = m_Matriz.Transforma(aux2).y;
	}
}

inline float Cena::GetF( char* addr )
{
	memcpy( alinhado, addr, 4 );
	return *(float*)alinhado;
}
inline unsigned short Cena::GetW( char* addr )
{
	memcpy( alinhado, addr, 2 );
	return *(unsigned short*)alinhado;
}
inline unsigned long Cena::GetD( char* addr )
{
	memcpy( alinhado, addr, 4 );
	return *(uint32_t*)alinhado;
}

long Cena::ComeLixo( char* buffer )
{
	short chunkid = GetW( buffer );
	long chunklength = GetD( buffer + 2 );
	int j, i = 6, cp = 0, tcoords = 0;
	switch (chunkid)
	{
	case 0x4D4D:
		while ((GetW( buffer + i) != 0x3D3D) && (GetW( buffer + i) != 0xB000)) i += 2;
		break;
	case 0x4000:
		while (*(buffer + (i++)) != 0);
		break;
	case 0x4110:
		m_NrVerts = GetW( buffer + i );
		delete m_Verts;
		m_Verts = new float[m_NrVerts * 3];
		i += 2;
		for ( j = 0; j < m_NrVerts; j++ )
		{
			m_Verts[j * 3] = GetF( buffer + i );
			m_Verts[j * 3 + 1] = GetF( buffer + i + 4 );
			m_Verts[j * 3 + 2] = GetF( buffer + i + 8 );
			i += 12;
		}
		break;
	case 0x4120:
		m_NrFaces = GetW( buffer + i );
		delete m_Faces;
		m_Faces = new unsigned short[m_NrFaces * 3];
		i += 2;
		for ( j = 0; j < m_NrFaces; j++ )
		{
			m_Faces[j * 3] = GetW( buffer + i );
			m_Faces[j * 3 + 1] = GetW( buffer + i + 2 );
			m_Faces[j * 3 + 2] = GetW( buffer + i + 4 );
			i += 8;
		}
	case 0x4140:
		tcoords = GetW( buffer + i );
		delete m_TCoords;
		m_TCoords = new float[tcoords * 2];
		i += 2;
		for ( j = 0; j < tcoords; j++ )
		{
			m_TCoords[j * 2] = GetF( buffer + i );
			m_TCoords[j * 2 + 1] = GetF( buffer + i + 4 );
			i += 8;
		}
	case 0x3D3D:
	case 0x4100:
		break;
	default:
		i = chunklength;
	break;
	}
	while (i < chunklength) i += ComeLixo( buffer + i );
	return chunklength;
}
//------------------------------------------------------------
// Efetua a leitura do arquivo 3DS
//------------------------------------------------------------
void Cena::Carrega3DS( ListaDDE * m_Triangulos, wxString a_Arquivo, Material* a_Material, vetor3& a_Pos, float a_Size )
{
	temp = new unsigned char[8];
	alinhado = (unsigned char*)(((unsigned long)temp + 4) & ~(uintptr_t)3);
	// carrega o arquivo na memória
	FILE* f = NULL;
	fopen_s(&f, a_Arquivo.c_str(), "r" );
	if(f == NULL)
	{
		wxString s;
		s.Printf("Impossível abrir arquivo %s", a_Arquivo.c_str());
		wxMessageBox(s);
	}
	char* memfile = new char[800 * 1024];
	unsigned long filelength = fread( memfile, 800 * 1024, 1, f );
	fclose( f );
	m_Verts = m_TCoords = NULL;
	m_Faces = 0;
	// process chunks
	ComeLixo( memfile );
	delete memfile;
	// determina o tamanho
	vetor3 min( vetor3( m_Verts[0], m_Verts[1], m_Verts[2] ) );
	vetor3 max = min;
	for ( int i = 1; i < m_NrVerts; i++ )
	{
		if (m_Verts[i * 3] < min.x) min.x = m_Verts[i * 3];
		if (m_Verts[i * 3] > max.x) max.x = m_Verts[i * 3];
		if (m_Verts[i * 3 + 1] < min.y) min.y = m_Verts[i * 3 + 1];
		if (m_Verts[i * 3 + 1] > max.y) max.y = m_Verts[i * 3 + 1];
		if (m_Verts[i * 3 + 2] < min.z) min.z = m_Verts[i * 3 + 2];
		if (m_Verts[i * 3 + 2] > max.z) max.z = m_Verts[i * 3 + 2];
	}
	vetor3 size = max - min;
	// determina a escala do triângulo
	float scale;
	if ((size.x > size.y) && (size.x > size.z)) scale = a_Size / size.x;
	else if (size.y > size.z) scale = a_Size / size.y; 
	else scale = a_Size / size.z;
	// determina o valor de offset
	vetor3 centre = (min + max) * 0.5f;
	vetor3 offset = (centre * scale) - a_Pos;
	Vertice** vert = new Vertice*[m_NrVerts];
	Primitiva*** vertface = new Primitiva**[m_NrVerts];
	int* vertfaces = new int[m_NrVerts];
	for ( int i = 0; i < m_NrVerts; i++ ) 
	{
		float x = m_Verts[i * 3], z = m_Verts[i * 3 + 1], y = m_Verts[i * 3 + 2];
		vert[i] = new Vertice( vetor3( (x * scale) - offset.x, (y * scale ) - offset.y, (z * scale) - offset.z ), 0, 0 );
		vert[i]->SetUV( vert[i]->GetPos().cell[0] / a_Size, vert[i]->GetPos().cell[1] / a_Size );
		vertface[i] = new Primitiva*[10];
		vertfaces[i] = 0;
	}

	Primitiva **m_Prim = new Primitiva*[20000];
	int m_Primitivas = 0;
	for ( int i = 0; i < m_NrFaces; i++ )
	{
		int idx[3];
		for ( int v = 0; v < 3; v++ ) 
			idx[v] = m_Faces[i * 3 + v];
		
		m_Prim[m_Primitivas] = new Triangulo(vert[idx[0]], vert[idx[1]], vert[idx[2]] );
		m_Prim[m_Primitivas]->SetNome("Malha Sem Nome");
		m_Prim[m_Primitivas++]->SetMaterial( a_Material );
		for ( int v = 0; v < 1; v++ ) 
			if (vertfaces[idx[v]] < 10) 
				vertface[idx[v]][vertfaces[idx[v]]++] = m_Prim[m_Primitivas - 1];
	}
	//joga pra pilha...
	for(int i=0; i< m_Primitivas; i++)
	{
		m_Triangulos->insereUltimoLDDE(&m_Prim[i]);
	}
	delete m_Prim;
	// calcula as normais dos vértices para a suavisação
	for ( int i = 0; i < m_NrVerts; i++ )
	{
		vetor3 N( 0, 0, 0 );
		for ( int v = 0; v < vertfaces[i]; v++ )
			N += vertface[i][v]->GetNormal();
		N *= 1.0f / vertfaces[i];
		vert[i]->SetNormal( N );
		delete vertface[i];
	}
	delete vertface;
	delete vertfaces;
	delete vert;
}

//------------------------------------------------------------
// Adiciona a primitiva à KD-Tree
//------------------------------------------------------------
void NoKdTree::Adiciona( Primitiva* a_Prim )
{
	//AAAAAAAAAAAAAAAAAAAAAAAAAAH!
	ListaObjeto* lnode = KdTree::s_MMemoria->NovaListaObjetos();
	lnode->SetPrimitiva( a_Prim );
	lnode->SetNext( GetLista() );
	SetLista( lnode );
}

//------------------------------------------------------------
// Construtor KD-Tree
//------------------------------------------------------------
KdTree::KdTree()
{
	m_Raiz = new NoKdTree();
}

//------------------------------------------------------------
// Constroi a KD-Tree
//------------------------------------------------------------
void KdTree::Constroi( Cena* a_Cena )
{
	m_Prof = 0;
	for ( int p = 0; p < a_Cena->GetNrPrimitivas(); p++ )
		m_Raiz->Adiciona( a_Cena->GetPrimitiva( p ) );
	int prims = a_Cena->GetNrPrimitivas();
	aabb sbox = a_Cena->GetExtends();
	m_SPool = new ListaDivisao[prims * 2 + 8];
	int i;
	for ( i = 0; i < (prims * 2 + 6); i++ ) 
		m_SPool[i].prox = &m_SPool[i + 1];
	m_SPool[i].prox = 0;
	m_SLista = 0;
	Subdivide( m_Raiz, sbox, 0, prims );
	/*wxString s;
	s.Printf("Profundidade: %i", m_Prof);
	wxMessageBox(s);*/
}

//------------------------------------------------------------
// Prepara a cena para ser passada como textura à GPU
//------------------------------------------------------------
void Cena::PreparaTextura()
{
	/*if(m_Data != NULL)
		free(m_Data);
	if(m_DataLuzes != NULL)
		free(m_DataLuzes);*/

	int es = 0;
	int tri = 0;
	for(int i=1; i<= m_Esferas->quantosDadosLDDE(); i++)
	{
		Primitiva *p;
		m_Esferas->buscaNaPosicaoLDDE(&p, i);
		if(!p->ELuz())
			es++;
	}

	for(int i=1; i<= m_Planos->quantosDadosLDDE(); i++)
	{
		Primitiva *p;
		m_Planos->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Plano *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			Primitiva *pp;
			((Plano *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(!pp->ELuz())
				tri++;
		}
	}

	m_TamTextura = (17 * es) + (44 * tri);
	if(m_TamTextura%4 != 0)
		m_TamTextura += (4 - m_TamTextura%4);

	int cont = 0;
	int bk1;
	int ultimo = -1;

	m_Data = new float[m_TamTextura];

	for(int i=1; i <= m_Esferas->quantosDadosLDDE(); i++)
	{
		Primitiva *p;
		m_Esferas->buscaNaPosicaoLDDE(&p, i);

		if(!p->ELuz())
		{
			m_Data[cont++] = ultimo;
			bk1 = cont++;
			m_Data[cont++] = 1;//ESFERA
			m_Data[cont++] = 0;//AUX

			m_Data[cont++] = ((Esfera *)p)->GetCentro().x;
			m_Data[cont++] = ((Esfera *)p)->GetCentro().y;
			m_Data[cont++] = ((Esfera *)p)->GetCentro().z;
			m_Data[cont++] = ((Esfera *)p)->GetRaio();
			
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().r;
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().g;
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().b;
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetReflexao();

			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetDiffuse();
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetSpecular();
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetRefracao();
			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetRefrIndex();

			m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetDiffuseRefl();
			m_Data[bk1] = cont;
		}
	}

	for(int i=1; i <= m_Planos->quantosDadosLDDE(); i++)
	{
		Primitiva *p;
		m_Planos->buscaNaPosicaoLDDE(&p, i);

		for(int j=1; j<= ((Plano *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			Primitiva *pp;
			((Plano *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);

			if(!pp->ELuz())
			{
				m_Data[cont++] = ultimo;
				bk1 = cont++;
				m_Data[cont++] = 2;//TRIANGULO
				m_Data[cont++] = 0;//AUX

				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(0)->GetPos().x;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(0)->GetPos().y;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(0)->GetPos().z;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(1)->GetPos().x;

				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(1)->GetPos().y;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(1)->GetPos().z;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(2)->GetPos().x;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(2)->GetPos().y;

				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(2)->GetPos().z;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(0)->GetNormal().x;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(0)->GetNormal().y;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(0)->GetNormal().z;

				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(1)->GetNormal().x;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(1)->GetNormal().y;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(1)->GetNormal().z;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(2)->GetNormal().x;

				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(2)->GetNormal().y;
				m_Data[cont++] = ((Triangulo *)pp)->GetVertice(2)->GetNormal().z;
				m_Data[cont++] = ((Triangulo *)pp)->GetU();
				m_Data[cont++] = ((Triangulo *)pp)->GetV();

				m_Data[cont++] = ((Triangulo *)pp)->GetN().x;
				m_Data[cont++] = ((Triangulo *)pp)->GetN().y;
				m_Data[cont++] = ((Triangulo *)pp)->GetN().z;
				m_Data[cont++] = ((Triangulo *)pp)->GetNU();

				m_Data[cont++] = ((Triangulo *)pp)->GetNV();
				m_Data[cont++] = ((Triangulo *)pp)->GetND();
				m_Data[cont++] = ((Triangulo *)pp)->GetBNU();
				m_Data[cont++] = ((Triangulo *)pp)->GetBNV();
				
				m_Data[cont++] = ((Triangulo *)pp)->GetCNU();
				m_Data[cont++] = ((Triangulo *)pp)->GetCNV();
				m_Data[cont++] = ((Triangulo *)pp)->GetK();				
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().r;

				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().g;
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().b;
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetReflexao();
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetDiffuse();

				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetSpecular();
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetRefracao();
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetRefrIndex();
				m_Data[cont++] = ((Esfera *)p)->GetMaterial()->GetDiffuseRefl();

				m_Data[bk1] = cont;
			}
		}
	}

	m_TamLuzes = (9 * ContaLuz());
	if(m_TamLuzes%4 != 0)
		m_TamLuzes += (4 - m_TamLuzes%4);
	cont = 0;
	ultimo = -1;

	m_DataLuzes = new float[m_TamLuzes];

	for(int i=1; i <= m_Esferas->quantosDadosLDDE(); i++)
	{
		Primitiva *p;
		m_Esferas->buscaNaPosicaoLDDE(&p, i);

		if(p->ELuz())
		{
			m_DataLuzes[cont++] = ultimo;
			bk1 = cont++;
			m_DataLuzes[cont++] = ((Esfera *)p)->GetCentro().x;
			m_DataLuzes[cont++] = ((Esfera *)p)->GetCentro().y;

			m_DataLuzes[cont++] = ((Esfera *)p)->GetCentro().z;
			m_DataLuzes[cont++] = ((Esfera *)p)->GetRaio();
			m_DataLuzes[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().r;
			m_DataLuzes[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().g;

			m_DataLuzes[cont++] = ((Esfera *)p)->GetMaterial()->GetCor().b;
			m_DataLuzes[bk1] = cont;
		}
	}
	for(int i= 0; i<=cont; i++)
	{
		float x = m_DataLuzes[i];
	}
}

//------------------------------------------------------------
// Retona a textura para ser passada à GPU
//------------------------------------------------------------
float* Cena::RetornaInfoTextura()
{
	return m_Data;
}

//------------------------------------------------------------
// Retorna a textura de luzes para ser passada à GPU
//------------------------------------------------------------
float* Cena::RetornaInfoTexturaLuzes()
{
	return m_DataLuzes;
}

int Cena::ContaPrim()
{
	int ret = 0;
	Primitiva *p;
	Primitiva *pp;
	for(int i=1; i<= m_Malhas->quantosDadosLDDE(); i++)
	{
		m_Malhas->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Malha *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			((Malha *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(!pp->ELuz())
				ret++;
		}
	}

	for(int i=1; i<= m_Planos->quantosDadosLDDE(); i++)
	{
		m_Planos->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Plano *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			((Plano *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(!pp->ELuz())
				ret++;
		}
	}

	for(int i=1; i<= m_Esferas->quantosDadosLDDE(); i++)
	{
		m_Esferas->buscaNaPosicaoLDDE(&p, i);
		if(!p->ELuz())
			ret++;
	}
	return ret;
}

int Cena::ContaLuz()
{
	int ret = 0;
	Primitiva *p;
	Primitiva *pp;
	for(int i=1; i<= m_Malhas->quantosDadosLDDE(); i++)
	{
		m_Malhas->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Malha *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			((Malha *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(pp->ELuz())
				ret++;
		}
	}

	for(int i=1; i<= m_Planos->quantosDadosLDDE(); i++)
	{
		m_Planos->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Plano *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			((Plano *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(pp->ELuz())
				ret++;
		}
	}

	for(int i=1; i<= m_Esferas->quantosDadosLDDE(); i++)
	{
		m_Esferas->buscaNaPosicaoLDDE(&p, i);
		if(p->ELuz())
			ret++;
	}
	return ret;
}

//------------------------------------------------------------
// Prepara a cena para ser renderizada
//------------------------------------------------------------
void Cena::PreparaCena()
{
	delete m_Primi;
	delete m_Luzes;
	m_NPrimi = ContaPrim();
	m_NLuzes = ContaLuz();

	m_Primi = new Primitiva*[m_NPrimi];
	m_Luzes = new Primitiva*[m_NLuzes];
	
	int auxP = 0, auxL = 0;
	Primitiva *p;
	Primitiva *pp;
	for(int i=1; i <= m_Malhas->quantosDadosLDDE(); i++)
	{
		m_Malhas->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Malha *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			((Malha *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(pp->ELuz())
				m_Luzes[auxL++] = pp;
			else
				m_Primi[auxP++] = pp;
		}
	}

	for(int i=1; i <= m_Planos->quantosDadosLDDE(); i++)
	{
		m_Planos->buscaNaPosicaoLDDE(&p, i);
		for(int j=1; j<= ((Plano *)p)->m_Triangulos->quantosDadosLDDE(); j++)
		{
			((Plano *)p)->m_Triangulos->buscaNaPosicaoLDDE(&pp, j);
			if(pp->ELuz())
				m_Luzes[auxL++] = pp;
			else
				m_Primi[auxP++] = pp;
		}
	}

	for(int i=1; i <= m_Esferas->quantosDadosLDDE(); i++)
	{
		m_Esferas->buscaNaPosicaoLDDE(&p, i);
		if(p->ELuz())
			m_Luzes[auxL++] = p;
		else
			m_Primi[auxP++] = p;
	}
}

//------------------------------------------------------------
// Insere uma posição de divisão na cena para a KD-Tree
//------------------------------------------------------------
void KdTree::InserePosicaoDivisao( float a_SplitPos )
{
	// insert a split position candidate in the list if unique
	ListaDivisao* entry = m_SPool;
	m_SPool = m_SPool->prox;
	entry->prox = 0;
	entry->posdiv = a_SplitPos;
	entry->contN1 = 0;
	entry->contN2 = 0;
	if (!m_SLista) m_SLista = entry; else
	{
		if (a_SplitPos < m_SLista->posdiv)
		{
			entry->prox = m_SLista;
			m_SLista = entry;
		}
		else if (a_SplitPos == m_SLista->posdiv)
		{
			entry->prox = m_SPool; // redundant; recycle
			m_SPool = entry;
		}
		else
		{
			ListaDivisao* list = m_SLista;
			while ((list->prox) && (a_SplitPos >= list->prox->posdiv)) 
			{
				if (a_SplitPos == list->prox->posdiv)
				{
					entry->prox = m_SPool; // redundant; recycle
					m_SPool = entry;
					return;
				}
				list = list->prox;
			}
			entry->prox = list->prox;
			list->prox = entry;
		}
	}
}

//------------------------------------------------------------
// Teste de intersecção Esfera
//------------------------------------------------------------
bool Esfera::IntersectaEsferaBox(  aabb& a_Box )
{
	float dmin = 0;
	vetor3 spos = m_Centro;
	vetor3 bpos = a_Box.GetPos();
	vetor3 bsize = a_Box.GetSize();
	for ( int i = 0; i < 3; i++ )
	{
		if (spos.cell[i] < bpos.cell[i]) 
		{
			dmin = dmin + (spos.cell[i] - bpos.cell[i]) * (spos.cell[i] - bpos.cell[i]);
		}
		else if (spos.cell[i] > (bpos.cell[i] + bsize.cell[i])) 
		{
			dmin = dmin + (spos.cell[i] - (bpos.cell[i] + bsize.cell[i])) * (spos.cell[i] - (bpos.cell[i] + bsize.cell[i]));
		}
	}
	return (dmin <= m_SqRaio);
}

#define FINDMINMAX( x0, x1, x2, min, max ) \
  min = max = x0; if(x1<min) min=x1; if(x1>max) max=x1; if(x2<min) min=x2; if(x2>max) max=x2;
// X-tests
#define AXISTEST_X01( a, b, fa, fb )											\
	p0 = a * v0.cell[1] - b * v0.cell[2], p2 = a * v2.cell[1] - b * v2.cell[2]; \
    if (p0 < p2) { min = p0; max = p2;} else { min = p2; max = p0; }			\
	rad = fa * a_BoxHalfsize.cell[1] + fb * a_BoxHalfsize.cell[2];				\
	if (min > rad || max < -rad) return 0;
#define AXISTEST_X2( a, b, fa, fb )												\
	p0 = a * v0.cell[1] - b * v0.cell[2], p1 = a * v1.cell[1] - b * v1.cell[2];	\
    if (p0 < p1) { min = p0; max = p1; } else { min = p1; max = p0;}			\
	rad = fa * a_BoxHalfsize.cell[1] + fb * a_BoxHalfsize.cell[2];				\
	if(min>rad || max<-rad) return 0;
// Y-tests
#define AXISTEST_Y02( a, b, fa, fb )											\
	p0 = -a * v0.cell[0] + b * v0.cell[2], p2 = -a * v2.cell[0] + b * v2.cell[2]; \
    if(p0 < p2) { min = p0; max = p2; } else { min = p2; max = p0; }			\
	rad = fa * a_BoxHalfsize.cell[0] + fb * a_BoxHalfsize.cell[2];				\
	if (min > rad || max < -rad) return 0;
#define AXISTEST_Y1( a, b, fa, fb )												\
	p0 = -a * v0.cell[0] + b * v0.cell[2], p1 = -a * v1.cell[0] + b * v1.cell[2]; \
    if (p0 < p1) { min = p0; max = p1; } else { min = p1; max = p0; }			\
	rad = fa * a_BoxHalfsize.cell[0] + fb * a_BoxHalfsize.cell[2];				\
	if (min > rad || max < -rad) return 0;
// Z-tests
#define AXISTEST_Z12( a, b, fa, fb )											\
	p1 = a * v1.cell[0] - b * v1.cell[1], p2 = a * v2.cell[0] - b * v2.cell[1]; \
    if(p2 < p1) { min = p2; max = p1; } else { min = p1; max = p2; }			\
	rad = fa * a_BoxHalfsize.cell[0] + fb * a_BoxHalfsize.cell[1];				\
	if (min > rad || max < -rad) return 0;
#define AXISTEST_Z0( a, b, fa, fb )												\
	p0 = a * v0.cell[0] - b * v0.cell[1], p1 = a * v1.cell[0] - b * v1.cell[1];	\
    if(p0 < p1) { min = p0; max = p1; } else { min = p1; max = p0; }			\
	rad = fa * a_BoxHalfsize.cell[0] + fb * a_BoxHalfsize.cell[1];				\
	if (min > rad || max < -rad) return 0;

bool Primitiva::SobrepoemPlanoBox( vetor3& a_Normal, vetor3& a_Vert, vetor3& a_MaxBox )
{
	vetor3 vmin, vmax;
	for( int q = 0; q < 3; q++ )
	{
		float v = a_Vert.cell[q];
		if (a_Normal.cell[q] > 0.0f)
		{
			vmin.cell[q] = -a_MaxBox.cell[q] - v;
			vmax.cell[q] =  a_MaxBox.cell[q] - v;
		}
		else
		{
			vmin.cell[q] =  a_MaxBox.cell[q] - v;
			vmax.cell[q] = -a_MaxBox.cell[q] - v;
		}
	}
	if (ESCALAR( a_Normal, vmin) > 0.0f) return false;
	if (ESCALAR( a_Normal, vmax) >= 0.0f) return true;
	return false;
}

//------------------------------------------------------------
// Teste de intersecção Box
//------------------------------------------------------------
bool Primitiva::IntersectaBox( aabb& a_Box )
{
	if (m_Tipo == ESFERA)
	{
		return ((Esfera *)this)->IntersectaEsferaBox(a_Box);
	}
	else
	{
		return ((Triangulo *)this)->IntersectaTriBox( a_Box.GetPos() + a_Box.GetSize() * 0.5f, a_Box.GetSize() * 0.5f);
	}
}

//------------------------------------------------------------
// Teste de intersecção Box-Triângulo
//------------------------------------------------------------
bool Triangulo::IntersectaTriBox( vetor3 a_BoxCentre, vetor3 a_BoxHalfsize )
{
	vetor3& a_V0 = m_Vertex[0]->GetPos();
	vetor3& a_V1 = m_Vertex[1]->GetPos();
	vetor3& a_V2 = m_Vertex[2]->GetPos();

	vetor3 v0, v1, v2, normal, e0, e1, e2;
	float min, max, p0, p1, p2, rad, fex, fey, fez;
	v0 = a_V0 - a_BoxCentre;
	v1 = a_V1 - a_BoxCentre;
	v2 = a_V2 - a_BoxCentre;
	e0 = v1 - v0, e1 = v2 - v1, e2 = v0 - v2;
	fex = fabsf( e0.cell[0] );
	fey = fabsf( e0.cell[1] );
	fez = fabsf( e0.cell[2] );
	AXISTEST_X01( e0.cell[2], e0.cell[1], fez, fey );
	AXISTEST_Y02( e0.cell[2], e0.cell[0], fez, fex );
	AXISTEST_Z12( e0.cell[1], e0.cell[0], fey, fex );
	fex = fabsf( e1.cell[0] );
	fey = fabsf( e1.cell[1] );
	fez = fabsf( e1.cell[2] );
	AXISTEST_X01( e1.cell[2], e1.cell[1], fez, fey );
	AXISTEST_Y02( e1.cell[2], e1.cell[0], fez, fex );
	AXISTEST_Z0 ( e1.cell[1], e1.cell[0], fey, fex );
	fex = fabsf( e2.cell[0] );
	fey = fabsf( e2.cell[1] );
	fez = fabsf( e2.cell[2] );
	AXISTEST_X2 ( e2.cell[2], e2.cell[1], fez, fey );
	AXISTEST_Y1 ( e2.cell[2], e2.cell[0], fez, fex );
	AXISTEST_Z12( e2.cell[1], e2.cell[0], fey, fex );
	FINDMINMAX( v0.cell[0], v1.cell[0], v2.cell[0], min, max );
	if (min > a_BoxHalfsize.cell[0] || max < -a_BoxHalfsize.cell[0]) return false;
	FINDMINMAX( v0.cell[1], v1.cell[1], v2.cell[1], min, max );
	if (min > a_BoxHalfsize.cell[1] || max < -a_BoxHalfsize.cell[1]) return false;
	FINDMINMAX( v0.cell[2], v1.cell[2], v2.cell[2], min, max );
	if (min > a_BoxHalfsize.cell[2] || max < -a_BoxHalfsize.cell[2]) return false;
	normal = e0.Cross( e1 );
	if (!SobrepoemPlanoBox( normal, v0, a_BoxHalfsize )) return false;
	return true;
}

//------------------------------------------------------------
// Subdivide a KD-Tree Tirado de Bikker 2008
//------------------------------------------------------------
void KdTree::Subdivide( NoKdTree* a_No, aabb& a_Box, int a_Prof, int a_Prims )
{
	m_Prof++;
	// recycle used split list nodes
	if (m_SLista)
	{
		ListaDivisao* list = m_SLista;
		while (list->prox) 
			list = list->prox;

		list->prox = m_SPool;
		m_SPool = m_SLista, m_SLista = 0;
	}

	// determine split axis
	vetor3 s = a_Box.GetSize();
	if((s.x >= s.y) && (s.x >= s.z)) 
		a_No->SetEixo( 0 );
	else 
		if((s.y >= s.x) && (s.y >= s.z))
			a_No->SetEixo( 1 );

	int axis = a_No->GetEixo();

	// make a list of the split position candidates
	ListaObjeto* l = a_No->GetLista();
	float p1, p2;
	float pos1 = a_Box.GetPos().cell[axis];
	float pos2 = a_Box.GetPos().cell[axis] + a_Box.GetSize().cell[axis];
	bool* pright = new bool[a_Prims];
	float* eleft = new float[a_Prims], *eright = new float[a_Prims];
	Primitiva** parray = new Primitiva*[a_Prims];
	int aidx = 0;
	while (l)
	{
		Primitiva* p = parray[aidx] = l->GetPrimitiva();
		pright[aidx] = true;
		p->CalculaIntervalo( eleft[aidx], eright[aidx], axis );
		aidx++;
		if (p->GetTipo() == Primitiva::ESFERA)
		{
			p1 = ((Esfera *)p)->GetCentro().cell[axis] - ((Esfera *)p)->GetRaio();
			p2 = ((Esfera *)p)->GetCentro().cell[axis] + ((Esfera *)p)->GetRaio();
			if ((p1 >= pos1) && (p1 <= pos2)) InserePosicaoDivisao( p1 );
			if ((p2 >= pos1) && (p2 <= pos2)) InserePosicaoDivisao( p2 );
		}
		else
		{
			for ( int i = 0; i < 3; i++ )
			{
				p1 = ((Triangulo*)p)->GetVertice( i )->GetPos().cell[axis];
				if ((p1 >= pos1) && (p1 <= pos2)) InserePosicaoDivisao( p1 );
			}
		}
		l = l->GetNext();
	}
	// determine contN1 / contN2 for each split position
	aabb b1, b2, b3 = a_Box, b4 = a_Box;
	ListaDivisao* splist = m_SLista;
	float b3p1 = b3.GetPos().cell[axis];
	float b4p2 = b4.GetPos().cell[axis] + b4.GetSize().cell[axis];
	while (splist)
	{
		b4.GetPos().cell[axis] = splist->posdiv;
		b4.GetSize().cell[axis] = pos2 - splist->posdiv;
		b3.GetSize().cell[axis] = splist->posdiv - pos1;
		float b3p2 = b3.GetPos().cell[axis] + b3.GetSize().cell[axis];
		float b4p1 = b4.GetPos().cell[axis];
		for ( int i = 0; i < a_Prims; i++ ) if (pright[i])
		{
			Primitiva* p = parray[i];
			if ((eleft[i] <= b3p2) && (eright[i] >= b3p1))
				if (p->IntersectaBox( b3 )) splist->contN1++;
			if ((eleft[i] <= b4p2) && (eright[i] >= b4p1))
				if (p->IntersectaBox( b4 )) splist->contN2++; else pright[i] = false;
		}
		else 
			splist->contN1++;
		splist = splist->prox;
	}

	delete pright;
	// calculate surface area for current node
	float SAV = 0.5f / (a_Box.w() * a_Box.d() + a_Box.w() * a_Box.h() + a_Box.d() * a_Box.h());
	// calculate cost for not splitting
	float Cleaf = a_Prims * 1.0f;
	// determine optimal split plane position
	splist = m_SLista;
	float lowcost = 10000;
	float bestpos = 0;
	while (splist)
	{
		// calculate child node extends
		b4.GetPos().cell[axis] = splist->posdiv;
		b4.GetSize().cell[axis] = pos2 - splist->posdiv;
		b3.GetSize().cell[axis] = splist->posdiv - pos1;
		// calculate child node cost
		float SA1 = 2 * (b3.w() * b3.d() + b3.w() * b3.h() + b3.d() * b3.h());
		float SA2 = 2 * (b4.w() * b4.d() + b4.w() * b4.h() + b4.d() * b4.h());
		float splitcost = 0.3f + 1.0f * (SA1 * SAV * splist->contN1 + SA2 * SAV * splist->contN2);
		// update best cost tracking variables
		if (splitcost < lowcost)
		{
			lowcost = splitcost;
			bestpos = splist->posdiv;
			b1 = b3, b2 = b4;
		}
		splist = splist->prox;
	}

	if (lowcost > Cleaf) 
		return;
	a_No->SetPosicaoDivisao( bestpos );
	// construct child nodes
	NoKdTree* left = s_MMemoria->NewKdTreeNodePair();
	int contN1 = 0, contN2 = 0, total = 0;
	// assign primitives to both sides
	float b1p1 = b1.GetPos().cell[axis];
	float b2p2 = b2.GetPos().cell[axis] + b2.GetSize().cell[axis];
	float b1p2 = b1.GetPos().cell[axis] + b1.GetSize().cell[axis];
	float b2p1 = b2.GetPos().cell[axis];
	for ( int i = 0; i < a_Prims; i++ )
	{
		Primitiva* p = parray[i];
		total++;
		if ((eleft[i] <= b1p2) && (eright[i] >= b1p1)) if (p->IntersectaBox( b1 )) 
		{
			left->Adiciona( p );
			contN1++;
		}
		if ((eleft[i] <= b2p2) && (eright[i] >= b2p1)) if (p->IntersectaBox( b2 )) 
		{
			(left + 1)->Adiciona( p );
			contN2++;
		}
	}
	delete eleft;
	delete eright;
	delete parray;
	s_MMemoria->LimpaListaObjetos( a_No->GetLista() );
	a_No->SetEsquerda( left );
	a_No->SetFolha( false );
	if (a_Prof < MAXTREEDEPTH)
	{
		if (contN1 > 2) Subdivide( left, b1, a_Prof + 1, contN1 );
		if (contN2 > 2) Subdivide( left + 1, b2, a_Prof + 1, contN2 );
	}
	else
		a_Prof;
}

//------------------------------------------------------------
// Carrega arquivo 3ds para a Malha
//------------------------------------------------------------
void Malha::CarregaArquivo(wxString a_Arquivo)
{
	RemoveMalha();
	m_Arquivo = a_Arquivo;
	m_Cena->Carrega3DS( m_Triangulos, m_Arquivo.c_str(), m_Material, m_Pos, m_Size);
}

//------------------------------------------------------------
// Construtor Malha
//------------------------------------------------------------
Malha::Malha(Cena *c, wxString a_Arquivo, Material* a_Material, vetor3& a_Pos, float a_Size )
{
	m_Triangulos = new ListaDDE(sizeof(Primitiva *));
	m_Cena = c;
	m_Arquivo = a_Arquivo;
	m_Material = a_Material;
	m_Pos = a_Pos;
	m_Size = a_Size;
	m_Cena->Carrega3DS( m_Triangulos, m_Arquivo.c_str(), m_Material, m_Pos, m_Size);
	m_Tipo = MALHA;
}

//------------------------------------------------------------
// Seta material da Malha
//------------------------------------------------------------
void Malha::SetMaterial(Material *a_Material)
{
	Primitiva *p;
	for(int i=1; i<= m_Triangulos->quantosDadosLDDE(); i++)
	{
		m_Triangulos->buscaNaPosicaoLDDE(&p, i);
		p->SetMaterial(a_Material);
	}
	m_Material = a_Material;
}

void Malha::RemoveMalha()
{
	m_Triangulos->reiniciaLDDE();
}

//------------------------------------------------------------
// Seta posição da Malha
//------------------------------------------------------------
void Malha::SetPos(vetor3 a_Pos)
{
	RemoveMalha();
	m_Pos = a_Pos;
	m_Cena->Carrega3DS(m_Triangulos, m_Arquivo.c_str(), m_Material, m_Pos, m_Size);
}

//------------------------------------------------------------
// Seta o tamanho da malha
//------------------------------------------------------------
void Malha::SetSize(float a_Size)
{
	RemoveMalha();
	m_Size = a_Size;
	m_Cena->Carrega3DS(m_Triangulos, m_Arquivo.c_str(), m_Material, m_Pos, m_Size);
}

int Textura::Modulo(int x)
{
	if(x < 0)
		return (-1 * x);
	return x;
}

//------------------------------------------------------------
// Carrega imagem para a Textura
//------------------------------------------------------------
Textura::Textura( wxString a_File )
{

	a_File = a_File.AfterLast('\\');
	wxStandardPaths *path = new wxStandardPaths();
	wxString caminhoApp = path->GetExecutablePath().BeforeLast('\\');
	a_File = a_File.Prepend("/scene-data/images/");
	a_File = a_File.Prepend(caminhoApp);

	wxString temp = a_File;
	temp.LowerCase();
	bool flag = true;
	wxImage *img = new wxImage();
	if(temp.EndsWith(".bmp"))
	{
		img->LoadFile(a_File.c_str(), wxBITMAP_TYPE_BMP);
		flag = false;
	}

	if(temp.EndsWith(".jpg"))
	{
		img->LoadFile(a_File.c_str(), wxBITMAP_TYPE_JPEG);
		flag = false;
	}

	if(temp.EndsWith(".png"))
	{
		img->LoadFile(a_File.c_str(), wxBITMAP_TYPE_PNG);
		flag = false;
	}

	if(temp.EndsWith(".gif"))
	{
		img->LoadFile(a_File.c_str(), wxBITMAP_TYPE_GIF);
		flag = false;
	}

	if(temp.EndsWith(".tga"))
	{
		img->LoadFile(a_File.c_str(), wxBITMAP_TYPE_TGA);
		flag = false;
	}

	if(flag)
	{
		img->LoadFile(a_File.c_str());
	}

	unsigned char *aux = (unsigned char *)img->GetData();
	m_Largura = img->GetWidth();
	m_Altura = img->GetHeight();
	int tam = img->GetWidth() * img->GetHeight();
	m_Imagem = new Cor[tam];

	float ad = 1.0f / 256;

	for(int i=0; i< tam; i++)
	{
		int p = i*3;
		int r = aux[p];
		int g = aux[p+1];
		int b = aux[p+2];

		m_Imagem[i] = Cor(r * ad, g * ad, b * ad);
	}
	int x = m_Imagem[0].r;
	m_File = a_File;
}

vetor3 Triangulo::GetNormal(vetor3 &a_Pos)
{
	vetor3 N1 = m_Vertex[0]->GetNormal();
	vetor3 N2 = m_Vertex[1]->GetNormal();
	vetor3 N3 = m_Vertex[2]->GetNormal();
	vetor3 N = N1 + m_U * (N2 - N1) + m_V * (N3 - N1);
	NORMALIZA( N );
	return N;
}

Cor Triangulo::GetCor(vetor3 &a_Pos)
{
	if (m_Material->GetTextura())
	{
		float U1 = m_Vertex[0]->GetU(), V1 = m_Vertex[0]->GetV();		
		float U2 = m_Vertex[1]->GetU(), V2 = m_Vertex[1]->GetV();		
		float U3 = m_Vertex[2]->GetU(), V3 = m_Vertex[2]->GetV();
		float u = U1 + m_U * (U2 - U1) + m_V * (U3 - U1);
		float v = V1 + m_U * (V2 - V1) + m_V * (V3 - V1);
		return m_Material->GetTextura()->GetTexel( u, v ) * m_Material->GetCor();
	}
	else
		return m_Material->GetCor();
}

//------------------------------------------------------------
// Construtor do Triângulo
//------------------------------------------------------------
Triangulo::Triangulo(Vertice* a_V1, Vertice* a_V2, Vertice* a_V3 )
{
	m_Tipo = TRIANGULO;
	m_Material = new Material();
	m_Vertex[0] = a_V1;
	m_Vertex[1] = a_V2;
	m_Vertex[2] = a_V3;
	// precomputa as normais
	vetor3 A = m_Vertex[0]->GetPos();
	vetor3 B = m_Vertex[1]->GetPos();
	vetor3 C = m_Vertex[2]->GetPos();
	vetor3 c = B - A;
	vetor3 b = C - A;
	m_N = b.Cross( c );
	int u, v;
	if (fabsf( m_N.x ) > fabsf( m_N.y))
	{
		if (fabsf( m_N.x ) > fabsf( m_N.z )) k = 0; else k = 2;
	}
	else
	{
		if (fabsf( m_N.y ) > fabsf( m_N.z )) k = 1; else k = 2;
	}
	u = (k + 1) % 3;
	v = (k + 2) % 3;
	// precomputa
	float krec = 1.0f / m_N.cell[k];
	nu = m_N.cell[u] * krec;
	nv = m_N.cell[v] * krec;
	nd = m_N.Escalar( A ) * krec;

	float reci = 1.0f / (b.cell[u] * c.cell[v] - b.cell[v] * c.cell[u]);
	bnu = b.cell[u] * reci;
	bnv = -b.cell[v] * reci;

	cnu = c.cell[v] * reci;
	cnv = -c.cell[u] * reci;
	// normaliza o triângulo
	m_N.Normaliza();
	m_Vertex[0]->SetNormal( m_N );
	m_Vertex[1]->SetNormal( m_N );
	m_Vertex[2]->SetNormal( m_N );
}

//------------------------------------------------------------
// Teste intersecção triângulo
//------------------------------------------------------------
unsigned int modulo[] = { 0, 1, 2, 0, 1 };
int Triangulo::Intersepta( Raio& a_Raio, float& a_Distancia )
{
	#define ku modulo[k + 1]
	#define kv modulo[k + 2]
	vetor3 O = a_Raio.GetOrigem();
	vetor3 D = a_Raio.GetDirecao();
	vetor3 A = m_Vertex[0]->GetPos();
	const float lnd = 1.0f / (D.cell[k] + nu * D.cell[ku] + nv * D.cell[kv]);
	const float t = (nd - O.cell[k] - nu * O.cell[ku] - nv * O.cell[kv]) * lnd;
	if (!(a_Distancia > t && t > 0)) 
		return MISS;
	float hu = O.cell[ku] + t * D.cell[ku] - A.cell[ku];
	float hv = O.cell[kv] + t * D.cell[kv] - A.cell[kv];
	float beta = m_U = hv * bnu + hu * bnv;
	if (beta < 0) 
		return MISS;
	float gamma = m_V = hu * cnu + hv * cnv;
	if (gamma < 0) 
		return MISS;
	if ((m_U + m_V) > 1) 
		return MISS;
	a_Distancia = t;
	return (ESCALAR( D, m_N ) > 0) ? INPRIM : HIT;
}

Esfera::Esfera()
{
	m_Raio = 0;
	m_Material = new Material();
	m_Vn = vetor3( 0, 1, 0 );
	m_Ve = vetor3( 1, 0, 0 );
	m_Vc = m_Vn.Cross( m_Ve );
	m_Tipo = ESFERA;
}


Esfera::Esfera( vetor3& a_Centro, float a_Raio )
{
	m_Centro = a_Centro;
	m_SqRaio = a_Raio * a_Raio;
	m_Raio = a_Raio;
	m_RRaio =  1.0f / a_Raio;
	m_Material = new Material();
	m_Vn = vetor3( 0, 1, 0 );
	m_Ve = vetor3( 1, 0, 0 );
	m_Vc = m_Vn.Cross( m_Ve );
	m_Tipo = ESFERA;
}

Cor Esfera::GetCor( vetor3& a_Pos )
{
	Cor retval;
	if (!m_Material->GetTextura())
		retval = m_Material->GetCor(); 
	else
	{
		vetor3 vp = (a_Pos - m_Centro) * m_RRaio;
		float phi = acosf( -ESCALAR( vp, m_Vn ) );
		float u, v = phi * m_Material->GetVScaleReci() * (1.0f / PI);
		float theta = (acosf( ESCALAR( m_Ve, vp ) / sinf( phi ))) * (2.0f / PI);
		if (ESCALAR( m_Vc, vp ) >= 0) u = (1.0f - theta) * m_Material->GetUScaleReci();
						     else u = theta * m_Material->GetUScaleReci();
		retval = m_Material->GetTextura()->GetTexel( u, v ) * m_Material->GetCor();
	}
	return retval;
}

//------------------------------------------------------------
// Retorna o Texel da textura
//------------------------------------------------------------
Cor Textura::GetTexel( float a_U, float a_V )
{
	// calcula interpolação bilinear
	float fu = (a_U + 1000.5f) * m_Largura;
	float fv = (a_V + 1000.0f) * m_Largura;
	int u1 = ((int)fu) % m_Largura;
	int v1 = ((int)fv) % m_Altura;
	int u2 = (u1 + 1) % m_Largura;
	int v2 = (v1 + 1) % m_Altura;
	// calcula frações utilizadas de u e v
	float fracu = fu - floorf( fu );
	float fracv = fv - floorf( fv );
	// calcula o peso de cada parte de u e v
	float w1 = (1 - fracu) * (1 - fracv);
	float w2 = fracu * (1 - fracv);
	float w3 = (1 - fracu) * fracv;
	float w4 = fracu *  fracv;
	// computa os quatro texels
	Cor c1 = m_Imagem[u1 + v1 * m_Largura];
	Cor c2 = m_Imagem[u2 + v1 * m_Largura];
	Cor c3 = m_Imagem[u1 + v2 * m_Largura];
	Cor c4 = m_Imagem[u2 + v2 * m_Largura];
	// média dos quatro texels
	return c1 * w1 + c2 * w2 + c3 * w3 + c4 * w4;
}

Cena::Cena() : m_Ambiente( 0, 0, 0 ), m_Fog(0), m_Extends( vetor3( 0, 0, 0 ), vetor3( 0, 0, 0 ) )
{
	m_Malhas = new ListaDDE(sizeof(Primitiva *));
	m_Planos = new ListaDDE(sizeof(Primitiva *));
	m_Esferas = new ListaDDE(sizeof(Primitiva *));
	m_Cubos = new ListaDDE(sizeof(Primitiva *));
	kdTreeGerada = false;
	m_Primi = new Primitiva*[1];
	m_Luzes = new Primitiva*[1];
	m_NPrimi = 0;
	m_NLuzes = 0;

	m_Data = NULL;
	m_DataLuzes = NULL;
}

// -----------------------------------------------------------
// Classe Material com as propriedades de cada material...
// -----------------------------------------------------------
Material::Material() :
	m_Cor( Cor( 1, 1, 1 ) ),	// Cor do mateiral
	m_Refl( 0 ),						// Reflexao do material
	m_Diff( 1 ),						// "Diffuse" do material
	m_Spec( 1 ),						// "Specular" do material
	m_Refr( 0 ),
	m_RIndex( 1.5f ),					// Indice de refração do material
	m_DRefl( 0 ),						// Reflexão difusa
	m_Textura( 0 ),
	m_UScale( 1.0f ),
	m_VScale( 1.0f )
{ 
	SetaEscalaUV(1.0f, 1.0f);
}

// -----------------------------------------------------------
// Classe Esfera (apenas uma esfera...)
// -----------------------------------------------------------

// Teste de intersecção...
int Esfera::Intersepta( Raio& a_Raio, float& a_Distancia )
{
	//Solução baseada em [1] página 39 seção 2.2
	//(1) Testa se a origem do raio está dentro da esfera...
	vetor3 oc = m_Centro - a_Raio.GetOrigem();
	float l2oc = ESCALAR(oc,oc);

	//(2) Acha a aproximação do raio com o centro da esfera...
	// essa aproximação se for > a siginifica que o raio aponta para a esfera.
	float tca = ESCALAR(oc,a_Raio.GetDirecao());
	
	if(l2oc < m_SqRaio && tca < 0)
	{
		//O raio está fora da esfera e aponta pra fora dela...
		//Sem chance de intersepção...
		return MISS;
	}
	else
	{
		//Calcula a distancia aproximada do raio ao centro da esfera...
		//Pitágoras!!!
		float t2hc = m_SqRaio - l2oc + (tca * tca);		
		if(t2hc <0)
		{
			//Nada feito, o raio não intersepta a esfera...
			return MISS;
		}
		else
		{
			
			t2hc = sqrtf( t2hc );
			float i1 = tca - t2hc;
			float i2 = tca + t2hc;
			if (i2 > 0)
			{
				if (i1 < 0) 
				{
					if (i2 < a_Distancia) 
					{
						a_Distancia = i2;
						return INPRIM; // O ponto está dentro da primitiva...
					}
				}
				else
				{
					if (i1 < a_Distancia)
					{
						a_Distancia = i1;
						return HIT; // O ponto está na prmitiva
					}
				}
			}		
		}
	}
	return MISS;
}

// -----------------------------------------------------------
// Classe Box (a caixinha)...
// -----------------------------------------------------------
int Box::Intersepta( Raio& a_Raio, float& a_Distancia )
{
	float dist[6];
	vetor3 ip[6];
	vetor3 d = a_Raio.GetDirecao();
	vetor3 o = a_Raio.GetOrigem();
	bool retval = MISS;
	for ( int i = 0; i < 6; i++ ) dist[i] = -1;
	vetor3 v1 = m_Pos, v2 = m_Pos + m_Size;
	if (d.x) 
	{
		float rc = 1.0f / d.x;
		dist[0] = (v1.x - o.x) * rc;
		dist[3] = (v2.x - o.x) * rc;
	}
	if (d.y) 
	{
		float rc = 1.0f / d.y;
		dist[1] = (v1.y - o.y) * rc;
		dist[4] = (v2.y - o.y) * rc;
	}
	if (d.z) 
	{
		float rc = 1.0f / d.z;
		dist[2] = (v1.z - o.z) * rc;
		dist[5] = (v2.z - o.z) * rc;
	}
	for (int i = 0; i < 6; i++ ) if (dist[i] > 0)
	{
		ip[i] = o + dist[i] * d;
		if ((ip[i].x > (v1.x - EPSELON)) && (ip[i].x < (v2.x + EPSELON)) && 
			(ip[i].y > (v1.y - EPSELON)) && (ip[i].y < (v2.y + EPSELON)) &&
			(ip[i].z > (v1.z - EPSELON)) && (ip[i].z < (v2.z + EPSELON)))
		{
			if (dist[i] < a_Distancia) 
			{
				a_Distancia = dist[i];
				retval = HIT;
			}
		}
	}
	return retval;
}

vetor3 Box::GetNormal( vetor3& a_Pos )
{
	float dist[6];
	dist[0] = (float)fabs( m_Size.x - m_Pos.x );
	dist[1] = (float)fabs( m_Size.x + m_Size.x - m_Pos.x );
	dist[2] = (float)fabs( m_Size.y - m_Pos.y );
	dist[3] = (float)fabs( m_Size.y + m_Size.y - m_Pos.y );
	dist[4] = (float)fabs( m_Size.z - m_Pos.z );
	dist[5] = (float)fabs( m_Size.z + m_Size.z - m_Pos.z );
	int best = 0;
	float bdist = dist[0];
	for ( int i = 1 ; i < 6; i++ ) if (dist[i] < bdist) 
	{ 
		bdist = dist[i]; 
		best = i;
	}
	if (best == 0) return vetor3( -1, 0, 0 );
	else if (best == 1) return vetor3( 1, 0, 0 );
	else if (best == 2) return vetor3( 0, -1, 0 );
	else if (best == 3)  return vetor3( 0, 1, 0 );
	else if (best == 4) return vetor3( 0, 0, -1 );
	else return vetor3( 0, 0, 1 );
}

// Construtor da "caixinha"
Box::Box( vetor3 a_Pos, vetor3 a_Size )
{	
	m_Size = a_Size;
	//centraliza o box...
	m_Pos= vetor3(a_Pos.x - (a_Size.x/2.0f), a_Pos.y - (a_Size.y/2.0f), a_Pos.z - (a_Size.z/2.0f));
}

// -----------------------------------------------------------
// Classe Cena (implementa... er... a cena!)
// -----------------------------------------------------------

// Destruidor (Pofff...)
Cena::~Cena()
{
	delete m_Malhas;
	delete m_Esferas;
	delete m_Planos;
	delete m_Cubos;
}

//Função que conta aquantidades de materiais definidos pelo usuário,
//uma vez que cada material é separado por uma ','...
//Ao menos deveria ser!
int Cena::quantMaterial(char * linha)
{
	int res=0;
	for(int i=0; i<=strlen(linha); i++)
		if(linha[i]==',')
			res++;
	return res;
}

Primitiva* Cena::GetPrimitiva( int a_Idx )
{
	return m_Primi[a_Idx];
}

Primitiva* Cena::GetLuz( int a_Idx )
{
	return m_Luzes[a_Idx];
}

void Cena::ConstroiKdTree()
{
	vetor3 p1, p2;
	p1 = vetor3( -14, -6, -6 ), p2 = vetor3( 14, 8, 30 );
	m_Extends = aabb( p1, p2 - p1 );
	m_KdTree = new KdTree();
	m_KdTree->Constroi( this );
	kdTreeGerada = true;
}

void Plano::SetPlano(vetor3 a_P1, vetor3 a_P2, vetor3 a_P3, vetor3 a_P4)
{
	RemovePlano();
	Vertice* v[4];
	v[0] = new Vertice( a_P1, 0, 0 );
	v[1] = new Vertice( a_P2, 0, 0 );
	v[2] = new Vertice( a_P3, 0, 0 );
	v[3] = new Vertice( a_P4, 0, 0 );
	Primitiva *p1, *p2;
	p1 = new Triangulo( v[0], v[1], v[3] );
	p1->SetMaterial(m_Material);
	p2 = new Triangulo( v[1], v[2], v[3] );
	p2->SetMaterial(m_Material);
	m_Triangulos->insereUltimoLDDE(&p1);
	m_Triangulos->insereUltimoLDDE(&p2);
}

vetor3 Plano::GetPos(int a_Idx)
{
	Primitiva *p;
	switch(a_Idx)
	{
	case 1:
		m_Triangulos->buscaNaPosicaoLDDE(&p, 1);
		return ((Triangulo *)p)->GetVertice(0)->GetPos();
		break;
	case 2:
		m_Triangulos->buscaNaPosicaoLDDE(&p, 1);
		return ((Triangulo *)p)->GetVertice(1)->GetPos();
		break;
	case 3:
		m_Triangulos->buscaNaPosicaoLDDE(&p, 2);
		return ((Triangulo *)p)->GetVertice(1)->GetPos();
		break;
	case 4:
		m_Triangulos->buscaNaPosicaoLDDE(&p, 2);
		return ((Triangulo *)p)->GetVertice(2)->GetPos();
		break;
	}
}

Plano::Plano(Cena *a_Cena) 
{
	m_Tipo = PLANO;
	m_Cena = a_Cena;
	m_Triangulos = new ListaDDE(sizeof(Primitiva *));
};

Plano::Plano(Cena *c, Material *a_Material, vetor3 a_P1, vetor3 a_P2, vetor3 a_P3, vetor3 a_P4)
{
	m_Cena = c;
	m_Material = a_Material;
	m_Triangulos = new ListaDDE(sizeof(Primitiva *));
	SetPlano( a_P1, a_P2, a_P3, a_P4 );
	m_Tipo = PLANO;
}

void Plano::SetMaterial(Material *a_Material)
{
	Primitiva *p;
	for(int i=1; i<= m_Triangulos->quantosDadosLDDE(); i++)
	{
		m_Triangulos->buscaNaPosicaoLDDE(&p, i);
		p->SetMaterial(a_Material);
	}
	m_Material = a_Material;
}

void Plano::RemovePlano()
{
	m_Triangulos->reiniciaLDDE();
}

Vertice::Vertice( vetor3 a_Pos, float a_U, float a_V ) : m_Pos( a_Pos ), m_U( a_U ), m_V( a_V ) 
{
	SetUV(GetPos().cell[0]/18.0f, GetPos().cell[1] / 18.0f);
}

void Cena::SalvaCena(wxString a)
{
	FILE *f = NULL;
	fopen_s(&f, a.c_str(), "wb");
	//Grava o cabeçalho...
	dCabecalho cabecalho;
	cabecalho.n_Esferas = m_Esferas->quantosDadosLDDE();
	cabecalho.n_Planos = m_Planos->quantosDadosLDDE();
	cabecalho.n_Malhas = m_Malhas->quantosDadosLDDE();
	fwrite(&cabecalho, sizeof(dCabecalho), 1, f);
	
	//Grava esferas...
	for(int i=1; i<= m_Esferas->quantosDadosLDDE(); i++)
	{
		dEsfera esfera;
		dMaterial material;

		Primitiva *p;
		m_Esferas->buscaNaPosicaoLDDE(&p, i);
		esfera.luz = p->ELuz();
		esfera.raio = ((Esfera *)p)->GetRaio();
		esfera.centro = ((Esfera *)p)->GetCentro();
		strcpy(esfera.nome, p->GetNome().c_str());
		fwrite(&esfera, sizeof(dEsfera), 1, f);
		
		material.m_Cor = p->GetMaterial()->GetCor();
		material.m_Diff = p->GetMaterial()->GetDiffuse();
		material.m_DRefl = p->GetMaterial()->GetDiffuseRefl();
		material.m_Refl = p->GetMaterial()->GetReflexao();
		material.m_Refr = p->GetMaterial()->GetRefracao();
		material.m_RIndex = p->GetMaterial()->GetRefrIndex();
		material.m_Spec = p->GetMaterial()->GetSpecular();
		material.m_UScale = p->GetMaterial()->GetUScale();
		material.m_VScale = p->GetMaterial()->GetVScale();
		if(p->GetMaterial()->GetTextura())
			strcpy(material.m_Textura, p->GetMaterial()->GetTextura()->GetFile().c_str());
		else
			strcpy(material.m_Textura, "");
		fwrite(&material, sizeof(dMaterial), 1, f);
	}

	//Grava planos...
	for(int i=1; i<= m_Planos->quantosDadosLDDE(); i++)
	{
		dPlano plano;
		dMaterial material;

		Primitiva *p;
		m_Planos->buscaNaPosicaoLDDE(&p, i);
		plano.luz = p->ELuz();
		plano.p1 = ((Plano *)p)->GetPos(1);
		plano.p2 = ((Plano *)p)->GetPos(2);
		plano.p3 = ((Plano *)p)->GetPos(3);
		plano.p4 = ((Plano *)p)->GetPos(4);
		strcpy(plano.nome, p->GetNome().c_str());
		fwrite(&plano, sizeof(dPlano), 1, f);
		
		material.m_Cor = p->GetMaterial()->GetCor();
		material.m_Diff = p->GetMaterial()->GetDiffuse();
		material.m_DRefl = p->GetMaterial()->GetDiffuseRefl();
		material.m_Refl = p->GetMaterial()->GetReflexao();
		material.m_Refr = p->GetMaterial()->GetRefracao();
		material.m_RIndex = p->GetMaterial()->GetRefrIndex();
		material.m_Spec = p->GetMaterial()->GetSpecular();
		material.m_UScale = p->GetMaterial()->GetUScale();
		material.m_VScale = p->GetMaterial()->GetVScale();
		if(p->GetMaterial()->GetTextura())
			strcpy(material.m_Textura, p->GetMaterial()->GetTextura()->GetFile().c_str());
		else
			strcpy(material.m_Textura, "");
		fwrite(&material, sizeof(dMaterial), 1, f);
	}

	//Grava malhas...
	for(int i=1; i<= m_Malhas->quantosDadosLDDE(); i++)
	{
		dMalha malha;
		dMaterial material;

		Primitiva *p;
		m_Malhas->buscaNaPosicaoLDDE(&p, i);
		malha.luz = p->ELuz();
		strcpy(malha.m_Arquivo, ((Malha *)p)->GetArquivo().c_str());
		malha.m_Pos = ((Malha *)p)->GetPos();
		malha.m_Size = ((Malha *)p)->GetSize();
		strcpy(malha.nome, p->GetNome().c_str());
		fwrite(&malha, sizeof(dMalha), 1, f);
		
		material.m_Cor = p->GetMaterial()->GetCor();
		material.m_Diff = p->GetMaterial()->GetDiffuse();
		material.m_DRefl = p->GetMaterial()->GetDiffuseRefl();
		material.m_Refl = p->GetMaterial()->GetReflexao();
		material.m_Refr = p->GetMaterial()->GetRefracao();
		material.m_RIndex = p->GetMaterial()->GetRefrIndex();
		material.m_Spec = p->GetMaterial()->GetSpecular();
		material.m_UScale = p->GetMaterial()->GetUScale();
		material.m_VScale = p->GetMaterial()->GetVScale();
		if(p->GetMaterial()->GetTextura())
			strcpy(material.m_Textura, p->GetMaterial()->GetTextura()->GetFile().c_str());
		else
			strcpy(material.m_Textura, "");
		fwrite(&material, sizeof(dMaterial), 1, f);
	}
	fclose(f);
}

void Cena::AbreCena(wxString a)
{
	FILE *f;
    fopen_s(&f, a.c_str(), "rb");
	//Lê o cabeçalho...
	dCabecalho cabecalho;
	fread(&cabecalho, sizeof(dCabecalho),1,f);

	//Lê esferas...
	m_Esferas->reiniciaLDDE();
    for(int i=0; i< cabecalho.n_Esferas; i++)
    {
		dEsfera esfera;
		dMaterial material;

		Primitiva *p = new Primitiva();
		fread(&esfera, sizeof(dEsfera), 1, f);
		p = new Esfera(esfera.centro, esfera.raio);
		p->SetNome(esfera.nome);
		p->Luz(esfera.luz);

		fread(&material, sizeof(dMaterial), 1, f);
		Material *m = new Material();
		m->SetCor(material.m_Cor);
		m->SetDiffuse(material.m_Diff);
		m->SetDiffuseRefl(material.m_DRefl);
		m->SetReflexao(material.m_Refl);
		m->SetRefracao(material.m_Refr);
		m->SetRefrIndex(material.m_RIndex);
		m->SetSpecular(material.m_Spec);
		m->SetaEscalaUV(material.m_UScale, material.m_VScale);
		if(strlen(material.m_Textura) > 0)
			m->SetTextura(new Textura(material.m_Textura));
		p->SetMaterial(m);
        if(!feof(f))
            m_Esferas->insereUltimoLDDE(&p);
    }

	//Lê planos...
	m_Planos->reiniciaLDDE();
    for(int i=0; i< cabecalho.n_Planos; i++)
    {
		dPlano plano;
		dMaterial material;

		Primitiva *p = new Primitiva();
		fread(&plano, sizeof(dPlano), 1, f);

		fread(&material, sizeof(dMaterial), 1, f);
		Material *m = new Material();
		m->SetCor(material.m_Cor);
		m->SetDiffuse(material.m_Diff);
		m->SetDiffuseRefl(material.m_DRefl);
		m->SetReflexao(material.m_Refl);
		m->SetRefracao(material.m_Refr);
		m->SetRefrIndex(material.m_RIndex);
		m->SetSpecular(material.m_Spec);
		m->SetaEscalaUV(material.m_UScale, material.m_VScale);
		if(strlen(material.m_Textura) > 0)
			m->SetTextura(new Textura(material.m_Textura));

		p = new Plano(this, m, plano.p1, plano.p2, plano.p3, plano.p4);
		p->SetNome(plano.nome);
		p->Luz(plano.luz);

        if(!feof(f))
            m_Planos->insereUltimoLDDE(&p);
    }

	//Lê malhas...
	m_Malhas->reiniciaLDDE();
    for(int i=0; i< cabecalho.n_Malhas; i++)
    {
		dMalha malha;
		dMaterial material;

		Primitiva *p = new Primitiva();
		fread(&malha, sizeof(dMalha), 1, f);

		fread(&material, sizeof(dMaterial), 1, f);
		Material *m = new Material();
		m->SetCor(material.m_Cor);
		m->SetDiffuse(material.m_Diff);
		m->SetDiffuseRefl(material.m_DRefl);
		m->SetReflexao(material.m_Refl);
		m->SetRefracao(material.m_Refr);
		m->SetRefrIndex(material.m_RIndex);
		m->SetSpecular(material.m_Spec);
		m->SetaEscalaUV(material.m_UScale, material.m_VScale);
		if(strlen(material.m_Textura) > 0)
			m->SetTextura(new Textura(material.m_Textura));
		
		wxString a_Arquivo;
		a_Arquivo.Printf("%s", malha.m_Arquivo);
		a_Arquivo =	a_Arquivo.AfterLast('\\');
		wxStandardPaths *path = new wxStandardPaths();
		wxString caminhoApp = path->GetExecutablePath().BeforeLast('\\');
		a_Arquivo = a_Arquivo.Prepend("/scene-data/meshes/");
		a_Arquivo = a_Arquivo.Prepend(caminhoApp);
		
		p = new Malha(this, a_Arquivo, m, malha.m_Pos, malha.m_Size);
		p->SetNome(malha.nome);
		p->Luz(malha.luz);

        if(!feof(f))
            m_Malhas->insereUltimoLDDE(&p);
    }
    fclose(f);
}

Cilindro::Cilindro(float a_Raio, float a_Altura, vetor3 a_Pos, float a_AngX, float a_AngY, float a_AngZ)
{
	m_Raio = a_Raio;
	m_Altura = a_Altura;
	m_Pos = a_Pos;
	m_AngX = a_AngX;
	m_AngY = a_AngY;
	m_AngZ = a_AngZ;
	m_Matriz.Rotaciona(m_Pos, m_AngX, m_AngY, m_AngZ);
	m_Material = new Material();
	m_Tipo = CILINDRO;
}

Cilindro::Cilindro()
{
	m_Raio = 0;
	m_Altura = 0;
	m_Pos = vetor3(0,0,0);
	m_AngX = 0;
	m_AngY = 0;
	m_AngZ = 0;
	m_Matriz.Rotaciona(m_Pos, m_AngX, m_AngY, m_AngZ);
	m_Material = new Material();
	m_Tipo = CILINDRO;
}

int Cilindro::Intersepta( Raio& a_Raio, float& a_Distancia )
{
	matriz transform, invTransform;
	float a, b, c, discrim;
	float t[4];
	bool valid[4];
	Raio tLinha;
	vetor3 iPonto[4];
	transform = m_Matriz;
	invTransform = m_Matriz;
	invTransform.Inverte();
	tLinha.SetDirecao(invTransform.Transforma(a_Raio.GetDirecao()));
	tLinha.SetOrigem(invTransform.Transforma(a_Raio.GetOrigem()));

	a = powf(tLinha.GetDirecao().x + tLinha.GetDirecao().y, 2);
	b = 2 * (tLinha.GetDirecao().x * tLinha.GetOrigem().x + tLinha.GetDirecao().y * tLinha.GetOrigem().y);
	c = powf(tLinha.GetOrigem().x, 2) + powf(tLinha.GetOrigem().y, 2) - powf(m_Raio, 2);
	discrim = b*b - 4*a*c;

	if(discrim >0 )
	{
		t[0] = (-b + sqrtf(discrim)) / (2 * a);
		t[1] = (-b - sqrtf(discrim)) / (2 * a);
		for(int i=0; i<=1; i++)
		{
			iPonto[i] = tLinha.GetOrigem() + t[i] * tLinha.GetDirecao();
			if(iPonto[i].z < 0 || iPonto[i].z > m_Altura)
				valid[i] = false;
			else
				valid[i] = true;
		}

		//Check end caps
		plano p[2];
		p[0].N = vetor3(0, 0, 1);
		p[1].N = vetor3(0, 0, -1);
		p[0].D = 0;
		p[1].D = m_Altura;
		for(int i=0; i<=1; i++)
		{
			if(InterseptaPlano(p[i], tLinha, t[i+3]))
			{
				iPonto[i+2] = tLinha.GetOrigem() + t[i+2] * tLinha.GetDirecao();
				float d = powf(iPonto[i+2].x, 2) + powf(iPonto[i+2].y, 2);
				if(d <= powf(m_Raio, 2))
					valid[i+2] = true;
				else
					valid[i+2] = false;
			}
			else
				valid[i+2] = false;
		}

		//Menor t
		float mint = FLT_MAX;
		int minIdx = 0;
		bool hit = false;
		for(int i=0; i<4; i++)
		{
			if(valid[i])
			{
				if(t[i] < mint)
				{
					mint = t[i];
					minIdx = i;
					hit = true;
				}
			}
		}

		a_Distancia = mint;
		return HIT;
	}
	else
	{
		t[0] = -b / (2 * a);
		iPonto[0] = tLinha.GetOrigem() + t[0] * tLinha.GetDirecao();
		if(iPonto[0].z < 0 || iPonto[0].z > m_Altura)
			return MISS;
		
		a_Distancia = t[0];
		return HIT;
	}
}

int Cilindro::InterseptaPlano( plano p, Raio& a_Raio, float& a_Distancia )
{
	float d = ESCALAR( p.N, a_Raio.GetDirecao() );
	if (d < 0)
	{
		float dist = -(ESCALAR( p.N, a_Raio.GetOrigem() ) + p.D) / d;
		if (dist < a_Distancia) 
		{
			a_Distancia = dist;
			return HIT;
		}
	}
	return MISS;
}

Cor Cilindro::GetCor( vetor3& a_Pos )
{
	Cor retval;
	//if (!m_Material->GetTextura())
		retval = m_Material->GetCor(); 
	/*else
	{
		vetor3 vp = (a_Pos - m_Centro) * m_RRaio;
		float phi = acosf( -ESCALAR( vp, m_Vn ) );
		float u, v = phi * m_Material->GetVScaleReci() * (1.0f / PI);
		float theta = (acosf( ESCALAR( m_Ve, vp ) / sinf( phi ))) * (2.0f / PI);
		if (ESCALAR( m_Vc, vp ) >= 0) u = (1.0f - theta) * m_Material->GetUScaleReci();
						     else u = theta * m_Material->GetUScaleReci();
		retval = m_Material->GetTextura()->GetTexel( u, v ) * m_Material->GetCor();
	}*/
	return retval;
}

vetor3 Cilindro::GetNormal(vetor3& a_Pos)
{
	return a_Pos;
}

bool Cilindro::IntersectaCilindroBox(aabb& a_Box)
{
	return false;
}

};