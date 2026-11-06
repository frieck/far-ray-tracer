#include "raytracer.h"
#include "Cena.h"
#include "Operacoes.h"
#include "memory.h"
#include <wx/msgdlg.h>

namespace Raytracer {

// -----------------------------------------------------------
// Classe Raio
// -----------------------------------------------------------

// Constroi o Raio
Raio::Raio( const vetor3& a_Origem, const vetor3& a_Direcao ) : m_Origem( a_Origem ), m_Direcao( a_Direcao )
{
}

// -----------------------------------------------------------
// Classe Engine
// -----------------------------------------------------------
Engine::Engine(Cena *cena)
{
	m_Cena = cena;
	KdTree::SetManipuladorMemoria( new ManipuladorMemoria() );
	m_ComKdTree = true;
	m_Mod = new int[64];
	m_Mod = (int*)((((unsigned long)m_Mod) + 32) & ~(uintptr_t)31);
	m_Mod[0] = 0, m_Mod[1] = 1, m_Mod[2] = 2, m_Mod[3] = 0, m_Mod[4] = 1;
	m_Stack = new pilhaKDTree[64];
	m_Stack = (pilhaKDTree*)((((unsigned long)m_Stack) + 32) & ~(uintptr_t)31);
	m_MaxProfRaio = 3; m_SScale = 1.0f / SAMPLES;
}

// Destroi a engine
Engine::~Engine()
{
}

// -----------------------------------------------------------
// Seta o alvo da engine.
// O plano onde os pixels serão desenhados...
// -----------------------------------------------------------
void Engine::SetAlvo(PixelData* a_Destino, int a_Largura, int a_Altura )
{
	m_Destino = a_Destino; // O buffer de pixels
	m_Largura = a_Largura;
	m_Altura = a_Altura;
}

//------------------------------------------------------------
// Função adaptada de Bikker 2008
//------------------------------------------------------------
int Engine::EncontraMaisProximo( Raio a_Raio, float& a_Dist, Primitiva*& a_Prim )
{
	float tnear = 0, tfar = a_Dist, t;
	int retval = 0;
	vetor3 p1 = m_Cena->GetExtends().GetPos();
	vetor3 p2 = p1 + m_Cena->GetExtends().GetSize();
	vetor3 D = a_Raio.GetDirecao(), O = a_Raio.GetOrigem();
	for ( int i = 0; i < 3; i++ ) if (D.cell[i] < 0) 
	{
		if (O.cell[i] < p1.cell[i]) return 0;
	}
	else if (O.cell[i] > p2.cell[i]) return 0;
	// clip ray segment to box
	for ( int i = 0; i < 3; i++ )
	{
		float pos = O.cell[i] + tfar * D.cell[i];
		if (D.cell[i] < 0)
		{
			// clip end point
			if (pos < p1.cell[i]) tfar = tnear + (tfar - tnear) * ((O.cell[i] - p1.cell[i]) / (O.cell[i] - pos));
			// clip start point
			if (O.cell[i] > p2.cell[i]) tnear += (tfar - tnear) * ((O.cell[i] - p2.cell[i]) / (tfar * D.cell[i]));
		}
		else
		{
			// clip end point
			if (pos > p2.cell[i]) tfar = tnear + (tfar - tnear) * ((p2.cell[i] - O.cell[i]) / (pos - O.cell[i]));
			// clip start point
			if (O.cell[i] < p1.cell[i]) tnear += (tfar - tnear) * ((p1.cell[i] - O.cell[i]) / (tfar * D.cell[i]));
		}
		if (tnear > tfar) return 0;
	}
	// inicia a pilha
	int entrypoint = 0, exitpoint = 1;
	// atravessa a KD-Tree
	NoKdTree* farchild, *currnode;
	currnode = m_Cena->GetKdTree()->GetRaiz();
	m_Stack[entrypoint].t = tnear;
	if (tnear > 0.0f) 
		m_Stack[entrypoint].pb = O + D * tnear;
	else 
		m_Stack[entrypoint].pb = O;
	m_Stack[exitpoint].t = tfar;
	m_Stack[exitpoint].pb = O + D * tfar;
	m_Stack[exitpoint].node = 0;
	// atravessa a KD-Tree
	while (currnode)
	{
		while (!currnode->EFolha())
		{
			float posdiv = currnode->GetPosicaoDivisao();
			int axis = currnode->GetEixo();
			if (m_Stack[entrypoint].pb.cell[axis] <= posdiv)
			{
				if (m_Stack[exitpoint].pb.cell[axis] <= posdiv)
				{
					currnode = currnode->GetEsquerda();
					continue;
				}
				if (m_Stack[exitpoint].pb.cell[axis] == posdiv)
				{
					currnode = currnode->GetDireita();
					continue;
				}
				currnode = currnode->GetEsquerda();
				farchild = currnode + 1; // GetDireita();
			}
			else
			{
				if (m_Stack[exitpoint].pb.cell[axis] > posdiv)
				{
					currnode = currnode->GetDireita();
					continue;
				}
				farchild = currnode->GetEsquerda();
				currnode = farchild + 1; // GetDireita();
			}
			t = (posdiv - O.cell[axis]) / D.cell[axis];
			int tmp = exitpoint++;
			if (exitpoint == entrypoint) exitpoint++;
			m_Stack[exitpoint].prev = tmp;
			m_Stack[exitpoint].t = t;
			m_Stack[exitpoint].node = farchild;
			m_Stack[exitpoint].pb.cell[axis] = posdiv;
			int nextaxis = m_Mod[axis + 1];
			int prevaxis = m_Mod[axis + 2];
			m_Stack[exitpoint].pb.cell[nextaxis] = O.cell[nextaxis] + t * D.cell[nextaxis];
			m_Stack[exitpoint].pb.cell[prevaxis] = O.cell[prevaxis] + t * D.cell[prevaxis];
		}
		ListaObjeto* list = currnode->GetLista();
		float dist = m_Stack[exitpoint].t;
		while (list)
		{
			Primitiva* pr = list->GetPrimitiva();
			int result;
			m_Intersectaions++;
			if (result = pr->Intersepta( a_Raio, dist ))
			{
				retval = result;
				a_Dist = dist;
				a_Prim = pr;
			}
			list = list->GetNext();
		}
		if (retval) return retval;
		entrypoint = exitpoint;
		currnode = m_Stack[exitpoint].node;
		exitpoint = m_Stack[entrypoint].prev;
	}
	return 0;
}

//------------------------------------------------------------
// Função adaptada de Bikker 2008
//------------------------------------------------------------
int Engine::EncontraOclusor( Raio a_Raio, float& a_Dist )
{
	float tnear = EPSELON, t;
	vetor3 O, D = a_Raio.GetDirecao();
	// inicia pilha
	int entrypoint = 0, exitpoint = 1;
	// atravessa a KD-Tree
	NoKdTree* farchild, *currnode = m_Cena->GetKdTree()->GetRaiz();
	m_Stack[entrypoint].t = tnear;
	m_Stack[entrypoint].pb = O = a_Raio.GetOrigem();
	m_Stack[exitpoint].t = a_Dist;
	m_Stack[exitpoint].pb = O + D * a_Dist;
	m_Stack[exitpoint].node = 0;
	// atravessa a KD-Tree
	while (currnode)
	{
		while (!currnode->EFolha())
		{
			float posdiv = currnode->GetPosicaoDivisao();
			int axis = currnode->GetEixo();
			if (m_Stack[entrypoint].pb.cell[axis] <= posdiv)
			{
				if (m_Stack[exitpoint].pb.cell[axis] <= posdiv)
				{
					currnode = currnode->GetEsquerda();
					continue;
				}
				if (m_Stack[exitpoint].pb.cell[axis] == posdiv)
				{
					currnode = currnode->GetDireita();
					continue;
				}
				currnode = currnode->GetEsquerda();
				farchild = currnode + 1; // GetDireita();
			}
			else
			{
				if (m_Stack[exitpoint].pb.cell[axis] > posdiv)
				{
					currnode = currnode->GetDireita();
					continue;
				}
				farchild = currnode->GetEsquerda();
				currnode = farchild + 1; // GetDireita();
			}
			t = (posdiv - O.cell[axis]) / D.cell[axis];
			int tmp = exitpoint;
			if (++exitpoint == entrypoint) exitpoint++;
			m_Stack[exitpoint].prev = tmp;
			m_Stack[exitpoint].t = t;
			m_Stack[exitpoint].node = farchild;
			m_Stack[exitpoint].pb.cell[axis] = posdiv;
			int nextaxis = m_Mod[axis + 1];
			int prevaxis = m_Mod[axis + 2];
			m_Stack[exitpoint].pb.cell[nextaxis] = O.cell[nextaxis] + t * D.cell[nextaxis];
			m_Stack[exitpoint].pb.cell[prevaxis] = O.cell[prevaxis] + t * D.cell[prevaxis];
		}
		ListaObjeto* list = currnode->GetLista();
		float dist = a_Dist; // m_Stack[exitpoint].t;
		while (list)
		{
			m_Intersectaions++;
			if (list->GetPrimitiva()->Intersepta( a_Raio, dist )) return 1;
			list = list->GetNext();
		}
		entrypoint = exitpoint;
		currnode = m_Stack[exitpoint].node;
		exitpoint = m_Stack[entrypoint].prev;
	}
	return 0;
}

//------------------------------------------------------------
// Calcula a colorização no ponto de encontro
//------------------------------------------------------------
float Engine::CalculaShade( Esfera* a_Light, vetor3 a_IP, vetor3& a_Dir, float a_Samples, float a_SScale )
{
	float retval;
	Primitiva* prim = 0;
	if (a_Light->GetTipo() == Primitiva::ESFERA)
	{
		// determina a iluminação
		retval = 0;
		a_Dir = a_Light->GetCentro() - a_IP;
		float tdist = COMPRIMENTO( a_Dir );
		a_Dir *= (1.0f / tdist);
		tdist *= 1 - 4 * EPSELON;
		m_RaysCast++;
		if (!EncontraOclusor( Raio( a_IP + a_Dir * EPSELON, a_Dir ), tdist )) return 1;
	}
	return retval;
}

Primitiva* Engine::RaytraceK( Raio a_Raio, Cor& a_Cor, int a_Profundidade, float a_RIndex, float& a_Distancia, float a_Samples, float a_SScale)
{
	a_Distancia = 10000.0f;
	Primitiva* prim = 0;
	int result;
	// encontra a intersecção mais próxima na KD-Tree
	if (!(result = EncontraMaisProximo( a_Raio, a_Distancia, prim ))) 
		return 0;
	// determina a cor no ponto de intersecção
	vetor3 pi = a_Raio.GetOrigem() + a_Raio.GetDirecao() * a_Distancia;
	Cor cor = prim->GetCor( pi );
	vetor3 N = prim->GetNormal( pi );
	// procura por luzes
	for ( int l = 0; l < m_Cena->GetNrLuzes(); l++ )
	{
		Primitiva* light = m_Cena->GetLuz( l );
		// computa iluminação no ponto de encontro
		vetor3 L;
		float shade = CalculaShade( (Esfera *)light, pi, L, a_Samples, a_SScale );
		if (shade > 0)
		{
			// calcula o shade difuso
			if (prim->GetMaterial()->GetDiffuse() > 0)
			{
				float dot = ESCALAR( L, N );
				if (dot > 0)
				{
					float diff = dot * prim->GetMaterial()->GetDiffuse() * shade;
					// Adiciona cor difusa à cor final
					a_Cor += diff * cor * light->GetCor( pi );
				}
			}
			// calcula a colorização especular
			if (prim->GetMaterial()->GetSpecular() > 0)
			{
				vetor3 R = L - 2.0f * ESCALAR( L, N ) * N;
				float dot = ESCALAR( a_Raio.GetDirecao(), R );
				if (dot > 0)
				{
					float spec = dot * prim->GetMaterial()->GetSpecular() * shade / (50 - 50 * dot + dot);
					// Adiciona a cor especular à cor final
					a_Cor += spec * light->GetCor( pi );
				}
			}
		}
	}
	// calcula a reflexão
	float refl = prim->GetMaterial()->GetReflexao();
	if ((refl > 0.0f) && (a_Profundidade < m_MaxProfRaio))
	{
		float drefl = prim->GetMaterial()->GetDiffuseRefl();
		if ((drefl > 0) && (a_Profundidade < 3))
		{
			// calcula a reflexão difusa
			vetor3 RP = a_Raio.GetDirecao() - 2.0f * ESCALAR( a_Raio.GetDirecao(), N ) * N;
			vetor3 RN1 = vetor3( RP.z, RP.y, -RP.x );
			vetor3 RN2 = RP.Cross( RN1 );
			refl *= a_SScale;
			for ( int i = 0; i < SAMPLES; i++ )
			{
				float xoffs, yoffs;
				do
				{
					xoffs = (m_Twister.Rand() - 0.5f) * drefl;
					yoffs = (m_Twister.Rand() - 0.5f) * drefl;
				}
				while ((xoffs * xoffs + yoffs * yoffs) > (drefl * drefl));
				vetor3 R = RP + RN1 * xoffs + RN2 * yoffs * drefl;
				NORMALIZA( R );
				float dist;
				Cor rcor( 0, 0, 0 );
				RaytraceK( Raio( pi + R * EPSELON, R ), rcor, a_Profundidade + 1, a_RIndex, dist, a_Samples * 0.25f, a_SScale * 4 );
				m_RaysCast++;
				a_Cor += refl * rcor * cor;
			}
		}
		else
		{
			// calcula a relfexão planar (normal)
			vetor3 N = prim->GetNormal( pi );
			vetor3 R = a_Raio.GetDirecao() - 2.0f * ESCALAR( a_Raio.GetDirecao(), N ) * N;
			Cor rcor( 0, 0, 0 );
			float dist;
			RaytraceK( Raio( pi + R * EPSELON, R ), rcor, a_Profundidade + 1, a_RIndex, dist, a_Samples * 0.5f, a_SScale * 2 );
			m_RaysCast++;
			a_Cor += refl * rcor * cor;
		}
	}
	// calcula a refração para materiais transparentes
	float refr = prim->GetMaterial()->GetRefracao();
	if ((refr > 0) && (a_Profundidade < m_MaxProfRaio))
	{
		float rindex = prim->GetMaterial()->GetRefrIndex();
		float n = a_RIndex / rindex;
		vetor3 N = prim->GetNormal( pi ) * (float)result;
		float cosI = -ESCALAR( N, a_Raio.GetDirecao() );
		float cosT2 = 1.0f - n * n * (1.0f - cosI * cosI);
		if (cosT2 > 0.0f)
		{
			vetor3 T = (n * a_Raio.GetDirecao()) + (n * cosI - sqrtf( cosT2 )) * N;
			Cor rcor( 0, 0, 0 );
			float dist;
			RaytraceK( Raio( pi + T * EPSELON, T ), rcor, a_Profundidade + 1, rindex, dist, a_Samples * 0.5f, a_SScale * 2 );
			m_RaysCast++;
			// absorção da luz
			Cor absor = prim->GetMaterial()->GetCor() * 0.15f * -dist;
			Cor transp = Cor( expf( absor.r ), expf( absor.g ), expf( absor.b ) );
			a_Cor += rcor * transp;
		}
	}
	// Retorna a primitiva interseptada pelo raio...
	// uma vez que se a primitiva for a mesma, o cálculo é desnecessário
	return prim;
}

// -----------------------------------------------------------
// Renderiza a cena.
// Pega a 1ª primitiva atingida pelo raio...
// -----------------------------------------------------------
Primitiva* Engine::Raytrace( Raio a_Raio, Cor& a_Cor, int a_Profundidade, float a_RIndex, float& a_Distancia )
{
	if (a_Profundidade > m_MaxProfRaio) // Atingiu a profundidade máxima...
		return 0; // Para o raio...

	a_Distancia = 1000000.0f;
	vetor3 pInter; // ponto de intersecção
	Primitiva* prim = 0;
	int result;

	float fog = m_Cena->getFog();
	Cor fogCor = m_Cena->getFogCor();

	// Acha a intersecção mais próxima
	for ( int s = 0; s < m_Cena->GetNrPrimitivas(); s++ )
	{
		Primitiva* pr = m_Cena->GetPrimitiva( s );
		int res;
		if (res = pr->Intersepta( a_Raio, a_Distancia )) 
		{
			prim = pr; // A primitiva que foi intereceptada...
			result = res; // 0 = nada, 1 = no objeto, -1 = dentro do objeto...
		}
	}
	// Se a primitiva for nula, significa o raio não atingiu nenhuma primitiva...
	// logo a cor retornada será a cor do ambiente.
	if (!prim) 
	{
		//É somada à cor um fog (tentativa a ser aprimorada...) 
		a_Cor = m_Cena->getCorAmbiente() +  (a_Distancia * fogCor * 0.1f * fog);
		return 0;
	}
	// Se for uma luz, o raio intersepta a luz e apenas retorna a cor dessa luz...
	if (prim->ELuz())
	{
		//É somada à cor um fog (tentativa a ser aprimorada...) 
		a_Cor = prim->GetMaterial()->GetCor() +  (a_Distancia * fogCor * 0.1f * fog);
		return prim;
	}
	else
	{
		// Determina-se a cor no ponto de intersecção
		pInter = a_Raio.GetOrigem() + a_Raio.GetDirecao() * a_Distancia;
		Cor cor = prim->GetCor( pInter );
		// Procura por raios de sombra ou raios de iluminação...
		for ( int l = 0; l < m_Cena->GetNrLuzes(); l++ )
		{
			Primitiva* p = m_Cena->GetLuz( l );
			// Testa se o raio consegue chegar a fonte de luz, sem colidir com outro objeto
			if (p->ELuz()) 
			{
				Primitiva* luz = p;
				float shade = 1.0f;

				// Gera um novo raio do ponto da colisão em direção à luz
				vetor3 L = ((Esfera*)luz)->GetCentro() - pInter;
				float tdist = COMPRIMENTO(L);
				L *= (1.0f / tdist);
				Raio r = Raio( pInter + L * EPSELON, L );
				for ( int s = 0; s < m_Cena->GetNrPrimitivas(); s++ )
				{
					Primitiva* pr = m_Cena->GetPrimitiva( s );
					// O raio intersepta um objeto antes de chegar na luz...
					if ((pr != luz) && (pr->Intersepta( r, tdist )))
					{
						// O raio
						//r = Raio((pInter + L * EPSELON)*tdist, L);

						shade = 0;

						// Propaga a luz se o objeto for transparente..
						// Melhorar a matemática...
						/*float refr = pr->GetMaterial()->GetRefracao();
						if ((refr > 0) && (a_Profundidade < m_MaxProfRaio))
						{
							float rindex = pr->GetMaterial()->GetRefrIndex();
							float n = a_RIndex / rindex;
							vetor3 N = pr->GetNormal( pInter ) * (float)result;
							float cosI = -ESCALAR( N, r.GetDirecao() );
							float cosT2 = 1.0f - n * n * (1.0f - cosI * cosI);
							if (cosT2 > 0.0f)
							{
								vetor3 T = (n * r.GetDirecao()) + (n * cosI - sqrtf( cosT2 )) * N;
								Cor rcol( 0, 0, 0 );
								float dist;
								Raytrace( r, rcol, a_Profundidade + 1, rindex, dist );
								// Absorção da luz...
								// Cor_do_material*e^-(d*c)
								Cor absor = pr->GetMaterial()->GetCor() * 0.15f * -dist;
								Cor transp = Cor( expf( absor.r ), expf( absor.g ), expf( absor.b ) ) * refr;
								a_Cor += rcol * refr/4.45f * transp * prim->GetMaterial()->GetCor() +  (tdist * fogCor * 0.1f * fog);
								break;
							}
						}*/
						break;
					}
				}
				//A luz é interseptada por um objeto, então calcula o shading difuso da luz nesse objeto
				if (shade > 0)
				{
					// Calcula o shading difuso
					vetor3 L = ((Esfera*)luz)->GetCentro() - pInter;
					NORMALIZA( L );
					vetor3 N = prim->GetNormal( pInter );
					if (prim->GetMaterial()->GetDiffuse() > 0)
					{
						float dot = ESCALAR( L, N );
						if (dot > 0)
						{
							float diff = dot * prim->GetMaterial()->GetDiffuse() * shade;
							a_Cor += diff * cor * luz->GetMaterial()->GetCor()  +  (a_Distancia * fogCor * 0.1f * fog);
						}
					}

					// Calcula também a cor especular...
					if (prim->GetMaterial()->GetSpecular() > 0)
					{
						vetor3 V = a_Raio.GetDirecao();
						vetor3 R = L - 2.0f * ESCALAR( L, N ) * N;
						float dot = ESCALAR( V, R );
						if (dot > 0)
						{
							float spec = powf( dot, 20 ) * prim->GetMaterial()->GetSpecular() * shade;
							a_Cor += spec * luz->GetMaterial()->GetCor();
						}
					}
				}
				else
				{
					// apenas adiciona o fog...
					a_Cor += (a_Distancia * fogCor * 0.1f * fog);
				}
			}
		}

		//Reflexão
		float refl = prim->GetMaterial()->GetReflexao();
		if ((refl > 0.0f) && (a_Profundidade < m_MaxProfRaio))
		{
			float drefl = prim->GetMaterial()->GetDiffuseRefl();
			if ((drefl > 0) && (a_Profundidade < 2))
			{
				// Reflexão difusa...
				vetor3 N = prim->GetNormal( pInter );
				vetor3 RP = a_Raio.GetDirecao() - 2.0f * ESCALAR( a_Raio.GetDirecao(), N ) * N;
				vetor3 RN1 = vetor3( RP.z, RP.y, -RP.x );
				vetor3 RN2 = RP.Cross( RN1 );
				refl *= m_SScale;

				// Gera um cone de raios randômicos e calcula a média de cor desses raios...				
				for ( int i = 0; i < SAMPLES; i++ )
				{
					float xoffs, yoffs;
					do
					{
						//Gera randômicos em espiral, para formar o cone de randômicos...
						xoffs = m_Twister.Rand() * drefl;
						yoffs = m_Twister.Rand() * drefl;
					}
					while ((xoffs * xoffs + yoffs * yoffs) > (drefl * drefl));
					vetor3 R = RP + RN1 * xoffs + RN2 * yoffs * drefl;
					NORMALIZA( R );
					float dist;
					Cor rcol( 0, 0, 0 );
					Raytrace( Raio( pInter + R * EPSELON, R), rcol, a_Profundidade + 1, a_RIndex, dist );
					//Cor corR = prim->GetMaterial()->GetCor();
					a_Cor += refl * rcol * cor;
				}
			}
			else
			{
				// Apenas reflexão normal
				vetor3 N = prim->GetNormal( pInter );
				vetor3 R = a_Raio.GetDirecao() - 2.0f * ESCALAR( a_Raio.GetDirecao(), N ) * N;
				Cor rcol(0,0,0);				
				float dist;
				Raytrace( Raio( pInter + R * EPSELON, R ), rcol, a_Profundidade + 1, a_RIndex, dist );
				a_Cor += refl * rcol * cor +  (a_Distancia * fogCor * 0.1f * fog);				
			}
		}
		//Fim reflexão...

		// Calcula refração
		float refr = prim->GetMaterial()->GetRefracao();
		if ((refr > 0) && (a_Profundidade < m_MaxProfRaio))
		{
			float rindex = prim->GetMaterial()->GetRefrIndex();
			float n = a_RIndex / rindex;
			vetor3 N = prim->GetNormal( pInter ) * (float)result;;
			float cosI = -ESCALAR( N, a_Raio.GetDirecao() );
			float cosT2 = 1.0f - n * n * (1.0f - cosI * cosI);
			if (cosT2 > 0.0f)
			{
				vetor3 T = (n * a_Raio.GetDirecao()) + (n * cosI - sqrtf( cosT2 )) * N;
				Cor rcol( 0, 0, 0 );
				float dist;
				Raytrace( Raio( pInter + T * EPSELON, T ), rcol, a_Profundidade + 1, rindex, dist );
				// Abosrção de luz dentro do objeto...
				Cor absorbance = prim->GetMaterial()->GetCor() * 0.15f * -dist;
				Cor transparency = Cor( expf( absorbance.r ), expf( absorbance.g ), expf( absorbance.b ) );
				a_Cor += rcol * transparency ;
			}
		}	
	}
	// Retorna a primitiva interseptada pelo raio...
	// uma vez que se a primitiva for a mesma, o cálculo é desnecessário
	return prim;
}

int Engine::CalculaTamanhoTile(int a_Size)
{
	if(m_Altura % a_Size !=0 ||
	   m_Largura % a_Size !=0)
	return CalculaTamanhoTile(a_Size-1);
	else
		return a_Size;
}

// -----------------------------------------------------------
// Engine::InitRender
// Inicializa a engine enviando o buffer da imagem a ser salva...
// -----------------------------------------------------------
void Engine::InitRender( vetor3& a_Pos, vetor3& a_Alvo )
{	
	m_Tiles = CalculaTamanhoTile(TILESIZE);
	m_CurrLine = 0;
	m_PPos = 0;
	// Prepara o olho da câmera
	m_Origem = vetor3( 0, 0, -5 );
	m_P1 = vetor3( -4,  3, 0 );
	m_P2 = vetor3(  4,  3, 0 );
	m_P3 = vetor3(  4, -3, 0 );
	m_P4 = vetor3( -4, -3, 0 );
	// calculate camera matrix
	vetor3 zA = a_Alvo - a_Pos;
	zA.Normaliza();
	vetor3 up( 0, 1, 0 );
	vetor3 xA = up.Cross( zA );
	vetor3 yA = xA.Cross( -zA );
	matriz m;
	m.cell[0] = xA.x, m.cell[1] = xA.y, m.cell[2] = xA.z;
	m.cell[4] = yA.x, m.cell[5] = yA.y, m.cell[6] = yA.z;
	m.cell[8] = zA.x, m.cell[9] = zA.y, m.cell[10] = zA.z;
	m.Inverte();
	m.cell[3] = a_Pos.x, m.cell[7] = a_Pos.y, m.cell[11] = a_Pos.z;
	
	m_DX = (m_P2 - m_P1) * (1.0f / m_Largura);
	m_DY = (m_P4 - m_P1) * (1.0f / m_Altura);
	// Configura render com Tiles
	m_CurrCol = 0;
	m_CurrRow = 0;
	m_XTiles = m_Largura / m_Tiles;
	m_YTiles = m_Altura / m_Tiles;

	m_Intersectaions = 0;
	m_RaysCast = 0;
}

// -----------------------------------------------------------
// Engine::Render
// Renderiza!
// -----------------------------------------------------------

int Engine::retornaProgresso()
{
	return int(dest * 100.0f / float(m_Altura * m_Largura));
}

Primitiva* Engine::RenderizaRaio( vetor3 a_Pos, Cor& a_Acc )
{
	vetor3 origem = vetor3( 0, 0, -5 );
	aabb e = m_Cena->GetExtends();
	vetor3 dir = a_Pos - origem;
	NORMALIZA( dir );
	Cor acc( 0, 0, 0 );
	Raio r( origem, dir );
	m_RaysCast++;
	float dist;
	// traça o raio primário para cada pixel da tela
	if(m_ComKdTree)
		return RaytraceK( r, a_Acc, 1, 1.0f, dist, SAMPLES, 1.0f / SAMPLES );
	else
		return Raytrace( r, a_Acc, 1, 1.0f, dist);

	return 0;
}

bool Engine::Render()
{
	// tile render!
	aabb e = m_Cena->GetExtends();
	int msecs = GetTickCount();

	int tx = m_CurrCol, ty = m_CurrRow;
	int tdest = tx * m_Tiles + (ty * m_Tiles) * m_Largura;
	vetor3 tdir = m_P1 + (float)(tx * m_Tiles) * m_DX + (float)(ty * m_Tiles) * m_DY;
	while (1)
	{
		dest = tdest;
		vetor3 ldir = tdir;
		// para cada pixel da tela...
		for ( int y = 0; y < m_Tiles; y++ )
		{
			vetor3 pdir = ldir;
			for ( int x = 0; x < m_Tiles; x++ )
			{
				Cor acc( 0, 0, 0 );
				Primitiva* prim = RenderizaRaio( pdir, acc );
				float red, green, blue;
				red = (acc.r * 256);
				green = (acc.g * 256);
				blue = (acc.b * 256);
				if (red > 255) red = 255;
				if (green > 255) green = 255;
				if (blue > 255) blue = 255;
				m_Destino->SetaValor(dest++ , wxColour(red, green, blue));
				pdir += m_DX;
			}
			ldir += m_DY;
			dest += (m_Largura - m_Tiles);
		}
		tdest += m_Tiles;
		tdir += m_DX * m_Tiles;
		if (++tx == m_XTiles)
		{
			tx = 0;
			ty++;
			tdest = ty * m_Tiles * m_Largura;
			tdir = m_P1 + (float)(ty * m_Tiles) * m_DY;
		}
		if (ty < m_YTiles)
		{
			if ((GetTickCount() - msecs) > 2000000000) 
			{
				m_CurrCol = tx;
				m_CurrRow = ty;
				return false;
			}
		}
		else
			break;
	}
	return true;
}

};