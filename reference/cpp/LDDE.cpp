#include "ldde.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

ListaDDE::ListaDDE(int tamInfo)
{
    if(tamInfo < 0)
        return;
    p = (pLDDE)malloc(sizeof(LDDE));
    if(p != NULL)
    {
        p->inicio = NULL;
        p->tamInfo = tamInfo;
    }    
    return;
}

int ListaDDE::inserePrimeiroLDDE(void *item){

    pNoLDDE aux;
    aux = (pNoLDDE)malloc(sizeof(NoLDDE));
    if(aux == NULL)
        return FRACASSO;
    aux->dado = (void*)malloc(p->tamInfo);
    if(aux->dado == NULL){
        free(aux);
        return FRACASSO;
    }    
    memcpy(aux->dado, item, p->tamInfo);
    aux->prox = p->inicio;
    aux->ant = NULL;
    p->inicio = aux;
    return SUCESSO;
}

int ListaDDE::insereUltimoLDDE(void *item){

    pNoLDDE aux, aux2;
    if(p->inicio == NULL)
        return inserePrimeiroLDDE(item);
    aux = (pNoLDDE)malloc(sizeof(NoLDDE));
    if(aux == NULL)
        return FRACASSO;
    aux->dado = (void*)malloc(p->tamInfo);
    if(aux->dado == NULL){
        free(aux);
        aux = NULL;
        return FRACASSO;
    }    
    memcpy(aux->dado, item, p->tamInfo);
    aux->prox = NULL;
    aux2 = p->inicio;
    while(aux2->prox != NULL)
        aux2 = aux2->prox;
    aux2->prox = aux;
    aux->ant = aux2;
    return SUCESSO;    
}

int ListaDDE::insereNaPosicaoLDDE(void *item, int posicao){
    
    int cont=1;
    pNoLDDE pre, pos, aux;
    if((p->inicio == NULL) || (posicao < cont))
        return FRACASSO;
    if(posicao == cont)
        return inserePrimeiroLDDE(item);
    aux = (pNoLDDE)malloc(sizeof(NoLDDE));
    if(aux == NULL)
        return FRACASSO;
    aux->dado = (void*)malloc(p->tamInfo);
    if(aux->dado == NULL){
        free(aux);
        aux = NULL;
        return FRACASSO;
    }
    memcpy(aux->dado, item, p->tamInfo);     
    cont++;
    pre = p->inicio;
    pos = pre->prox;
    while((pos != NULL) && (cont < posicao)){
        pre = pre->prox;
        pos = pos->prox;
        cont++;
    }
    if(pos == NULL)
        return insereUltimoLDDE(item);
    aux->prox = pos;
    aux->ant = pre;
    pre->prox = aux;
    return SUCESSO;        
}

int ListaDDE::removePrimeiroLDDE(){
    
    pNoLDDE aux;
    if(p->inicio == NULL)
        return FRACASSO;
    aux = p->inicio;
    p->inicio = aux->prox;
    if(p->inicio != NULL)
        aux->prox->ant = NULL;
    free(aux->dado);
    free(aux);
    aux = NULL;
    return SUCESSO;
}

int ListaDDE::removeUltimoLDDE(){
    
    pNoLDDE aux,aux2;
    if(p->inicio == NULL)
        return FRACASSO;
    aux = p->inicio;
    if(aux->prox == NULL)
    {
        free(aux->dado);
        free(aux);
        p->inicio=NULL;
        return SUCESSO;
    }
    aux2=aux->prox;
    while(aux->prox->prox != NULL)
    {
        aux=aux2;
        aux2=aux2->prox;
    }
    free(aux->prox->dado);
    free(aux->prox);
    aux->prox=NULL;
    return SUCESSO;
}

int ListaDDE::removeNaPosicaoLDDE(int posicao){
    int cont = 1;
    pNoLDDE pre, pos;
    if((p->inicio == NULL) || (posicao < cont))
        return FRACASSO;
    if(posicao == cont)
        return removePrimeiroLDDE();
    if(posicao == quantosDadosLDDE())
        return removeUltimoLDDE();
    cont++;
    pre = p->inicio;
    pos = pre->prox;
    while((pos != NULL) && (cont < posicao)){
        pre = pre->prox;
        pos = pos->prox;
        cont++;
    }
    if(pos == NULL)
        return FRACASSO;
    pos->prox->ant = pre;
    pre->prox = pos->prox;
    free(pos->dado);
    return SUCESSO;
}

int ListaDDE::buscaPrimeiroLDDE(void *item){
    
    if(p->inicio != NULL){
        memcpy(item, p->inicio->dado, p->tamInfo);
        return SUCESSO;
    }else
        return FRACASSO;
}

int ListaDDE::buscaUltimoLDDE(void *item){
    
    pNoLDDE aux;
    if(p->inicio == NULL)
        return FRACASSO;
    aux = p->inicio;
    while(aux->prox != NULL)
        aux = aux->prox;                                    
    memcpy(item, aux->dado, p->tamInfo);
    return SUCESSO;
}

int ListaDDE::buscaNaPosicaoLDDE(void *item, int posicao){
    
    int cont = 1;
    pNoLDDE aux;
    if((p->inicio == NULL) || (posicao < cont))
        return FRACASSO;
    if(posicao == cont)
        return buscaPrimeiroLDDE(item);
    aux = p->inicio;
    while((aux->prox != NULL) && (cont < posicao)){
        aux = aux->prox;
        cont++;
    }
    if((aux == NULL) || (posicao != cont))
        return FRACASSO;
    memcpy(item, aux->dado, p->tamInfo);
    return SUCESSO;
}

int ListaDDE::testaVaziaLDDE(){
    if(p->inicio == NULL)
        return SUCESSO;
    else
        return FRACASSO;    
}

int ListaDDE::quantosDadosLDDE(){
    
    int cont = 0;
    pNoLDDE aux;
    if(p->inicio == NULL)
        return cont;
    aux = p->inicio;
    while(aux != NULL){
        aux = aux->prox;
        cont++;
    }
    return cont;    
}

void ListaDDE::reiniciaLDDE(){
    
    while(testaVaziaLDDE() == FRACASSO)
        removePrimeiroLDDE();
}

ListaDDE::~ListaDDE(){

    reiniciaLDDE();
    free(p);
    p = NULL;
}
