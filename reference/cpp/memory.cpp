// -----------------------------------------------------------
// memory.h
// 2004 - Jacco Bikker - jacco@bik5.com - www.bik5.com -   <><
// -----------------------------------------------------------

#include "memory.h"
#include "Cena.h"

namespace Raytracer {

ManipuladorMemoria::ManipuladorMemoria() : m_OList( 0 )
{
	// Constroi um array de  KdTreeNodes alinhados
	m_KdArray = (char*)(new NoKdTree[1000000]);
	m_ObjArray = (char*)(new ListaObjeto[100000]);
	unsigned long addr = (unsigned long)m_KdArray;
	m_KdPtr = (NoKdTree*)((addr + 32) & ~(uintptr_t)31);
	addr = (unsigned long)m_ObjArray;
	m_ObjPtr = (ListaObjeto*)((addr + 32) & ~(uintptr_t)31);
	ListaObjeto* ptr = m_ObjPtr;
	for ( int i = 0; i < 99995; i++ ) 
	{
		ptr->SetNext( ptr + 1 );
		ptr++;
	}
	ptr->SetNext( 0 );
	m_OList = m_ObjPtr;
}

ListaObjeto* ManipuladorMemoria::NovaListaObjetos()
{
	ListaObjeto* retval;
	retval = m_OList;
	m_OList = m_OList->GetNext();
	retval->SetNext( 0 );
	retval->SetPrimitiva( 0 );
	return retval;
}

void ManipuladorMemoria::LimpaListaObjetos( ListaObjeto* a_List )
{
	ListaObjeto* list = a_List;
	while (list->GetNext()) list = list->GetNext();
	list->SetNext( m_OList );
	m_OList = a_List;
}

NoKdTree* ManipuladorMemoria::NewKdTreeNodePair()
{ 
	unsigned long* tmp = (unsigned long*)m_KdPtr;
	tmp[1] = tmp[3] = 6;
	NoKdTree* node = m_KdPtr;
	m_KdPtr += 2;
	return node;
}

};