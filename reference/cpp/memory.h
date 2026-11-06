// -----------------------------------------------------------
// memory.h
// 2004 - Jacco Bikker - jacco@bik5.com - www.bik5.com -   <><
// -----------------------------------------------------------

#ifndef I_MEMORY_H
#define I_MEMORY_H

namespace Raytracer {

class ListaObjeto;
class NoKdTree;
class ManipuladorMemoria
{
public:
	ManipuladorMemoria();
	ListaObjeto* NovaListaObjetos();
	void LimpaListaObjetos( ListaObjeto* a_List );
	NoKdTree* NewKdTreeNodePair();
private:
	ListaObjeto* m_OList;
	char* m_KdArray, *m_ObjArray;
	NoKdTree* m_KdPtr;
	ListaObjeto* m_ObjPtr;
	int m_KdUsada;
};

};

#endif