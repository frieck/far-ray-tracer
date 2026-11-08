#!/usr/bin/env python3
"""Build .far JSON scenes. Functions return editable dictionaries.

    from far_gen import sphere, plane, mesh, save
    save('mine.far', spheres=[sphere((0,6,2), .1, light=True),
                             sphere((0,0,5), 1.5, color=(.9,.2,.2))])
"""
import json
import sys


def material(color=(1, 1, 1), refl=0, diff=1, spec=1, refr=0, ior=1.5,
             drefl=0, texture='', scale_u=1, scale_v=1):
    return dict(color=color, reflection=refl, diffuse=diff, specular=spec,
                refraction=refr, ior=ior, glossy=drefl, texture=texture,
                scale_u=scale_u, scale_v=scale_v)


def sphere(center, radius, light=False, **mat):
    if light:
        return dict(position=center, radius=radius, material=material(**mat))
    return dict(type='sphere', center=center, radius=radius, material=material(**mat))


def plane(p1, p2, p3, p4, **mat):
    return dict(type='plane', points=[p1, p2, p3, p4], material=material(**mat))


def mesh(file, pos, size, **mat):
    return dict(type='mesh', file=file, position=pos, size=size, material=material(**mat))


def save(path, spheres=(), planes=(), meshes=(), lights=(), objects=()):
    scene = dict(lights=[*lights, *(s for s in spheres if 'type' not in s)],
                 objects=[*meshes, *planes, *(s for s in spheres if 'type' in s), *objects])
    with open(path, 'w', encoding='utf-8') as output:
        json.dump(scene, output, indent=2, allow_nan=False)
        output.write('\n')


if __name__ == '__main__':
    if len(sys.argv) != 2:
        sys.exit('usage: far_gen.py output.far')
    save(sys.argv[1], spheres=[sphere((0,6,2), .1, light=True),
                              sphere((-1.5,-.5,5), 1.5, color=(.9,.2,.2), refl=.3),
                              sphere((1.8,-.8,4), 1.2, color=(.2,.5,.9), refl=.6)],
         planes=[plane((-8,-2,0), (8,-2,0), (8,-2,16), (-8,-2,16), refl=.2)])
