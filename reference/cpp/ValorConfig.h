#ifndef __VALORCONFIG_h__
#define __VALORCONFIG_h__

namespace Raytracer {

class ValorConfig
{
public:
	ValorConfig() : profundidadeRec(4){ kdtree = true;};
	~ValorConfig();
	int profundidadeRec;
	bool kdtree;
};

};

#endif