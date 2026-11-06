#ifndef _LDDE_H_
#define _LDDE_H_

#define FRACASSO 0
#define SUCESSO 1

class ListaDDE
{
      private:
          typedef struct NoLDDE
          {
              void *dado;
              struct NoLDDE *prox;
              struct NoLDDE *ant;
          }NoLDDE, *pNoLDDE;

          

          typedef struct 
          {
              int tamInfo;
              pNoLDDE inicio;
          }LDDE, *pLDDE, **ppLDDE;
          
          pLDDE p;
          
      public:          
                    
          //LDDE *pLDDE, **ppLDDE;
                       
          ListaDDE(int tamInfo);
          ~ListaDDE();
          int inserePrimeiroLDDE(void *item);
          int insereUltimoLDDE(void *item);
          int insereNaPosicaoLDDE(void *item, int posicao);
          int removePrimeiroLDDE();
          int removeUltimoLDDE();
          int removeNaPosicaoLDDE(int posicao);
          int buscaPrimeiroLDDE(void *item);
          int buscaUltimoLDDE(void *item);
          int buscaNaPosicaoLDDE(void *item, int posicao);
          int testaVaziaLDDE();
          int quantosDadosLDDE();
          void reiniciaLDDE();       
      
};


#endif

