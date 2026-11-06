#define mtRand_N 624


class Twister
{
public:
	Twister( uint32_t seed = (uint32_t)0xf2710812)
	{
	    Seed(seed); 
	}
	void Seed(uint32_t seed);	
	float Rand();
	uint32_t RandL();
protected:
	 uint32_t mt[mtRand_N];
	 int mti;
};
