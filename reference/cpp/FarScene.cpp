// JSON scene adapter for the 2008 reference renderer. Rendering code stays unchanged.
#include <fstream>
#include <stdexcept>
#include "vendor/json.hpp"
#include "Cena.h"
#include "wx/stdpaths.h"
using nlohmann::json;
using namespace Raytracer;

namespace {
vetor3 point(const json& value) {
    if (!value.is_array() || value.size() != 3) throw std::runtime_error("Expected a three-component point");
    return vetor3(value.at(0).get<float>(), value.at(1).get<float>(), value.at(2).get<float>());
}
std::string basename(std::string name) {
    auto at = name.find_last_of("/\\");
    return at == std::string::npos ? name : name.substr(at + 1);
}
Material* material(const json& data) {
    if (data.contains("surface") && !data["surface"].is_null()) throw std::runtime_error("Modern surface materials require the Enhanced Rust renderer");
    auto* m = new Material();
    auto color = point(data.value("color", json::array({1, 1, 1})));
    m->SetCor(color);
    m->SetDiffuse(data.value("diffuse", 1.f));
    m->SetDiffuseRefl(data.value("glossy", 0.f));
    m->SetReflexao(data.value("reflection", 0.f));
    m->SetRefracao(data.value("refraction", 0.f));
    m->SetRefrIndex(data.value("ior", 1.5f));
    m->SetSpecular(data.value("specular", 1.f));
    m->SetaEscalaUV(data.value("scale_u", 1.f), data.value("scale_v", 1.f));
    std::string texture = basename(data.value("texture", std::string()));
    if (!texture.empty()) m->SetTextura(new Textura(texture.c_str()));
    return m;
}
}

void Cena::AbreCena(wxString filename) {
    std::ifstream stream(filename.c_str());
    if (!stream) throw std::runtime_error("Cannot open scene: " + std::string(filename.c_str()));
    json scene; stream >> scene;
    if (scene.contains("environment") && !scene["environment"].is_null()) throw std::runtime_error("HDR environments require the Enhanced Rust renderer");
    // Fail explicitly instead of silently dropping features absent in the 2008 engine.
    for (const auto& object : scene.at("objects")) {
        std::string type = object.at("type");
        if (type != "sphere" && type != "plane" && type != "mesh")
            throw std::runtime_error("2008 reference does not implement object type: " + type);
    }
    for (const auto& light : scene.at("lights")) {
        if (light.contains("type")) throw std::runtime_error("2008 reference only implements spherical lights");
    }
    if (scene.contains("bounds")) {
        auto low = point(scene.at("bounds").at(0));
        auto high = point(scene.at("bounds").at(1));
        m_Extends = aabb(low, high - low);
    }
    m_Esferas->reiniciaLDDE(); m_Planos->reiniciaLDDE(); m_Malhas->reiniciaLDDE();
    for (const auto& light : scene.at("lights")) {
        auto center = point(light.at("position"));
        Primitiva* p = new Esfera(center, light.value("radius", 0.2f));
        json mat = light.value("material", json::object());
        if (!light.contains("material")) mat["color"] = light.value("color", json::array({1, 1, 1}));
        p->SetMaterial(material(mat)); p->Luz(true);
        m_Esferas->insereUltimoLDDE(&p);
    }
    for (const auto& object : scene.at("objects")) {
        auto* m = material(object.value("material", json::object()));
        std::string type = object.at("type");
        Primitiva* p;
        if (type == "sphere") {
            auto center = point(object.at("center"));
            p = new Esfera(center, object.at("radius").get<float>());
            p->SetMaterial(m); p->Luz(false); m_Esferas->insereUltimoLDDE(&p);
        } else if (type == "plane") {
            auto points = object.at("points");
            p = new Plano(this, m, point(points.at(0)), point(points.at(1)), point(points.at(2)), point(points.at(3)));
            p->Luz(false); m_Planos->insereUltimoLDDE(&p);
        } else {
            std::string name = basename(object.at("file").get<std::string>());
            wxStandardPaths paths;
            wxString root = paths.GetExecutablePath().BeforeLast('\\');
            std::string path = std::string(root.c_str()) + "/scene-data/meshes/" + name;
            vetor3 pos = point(object.at("position"));
            p = new Malha(this, path.c_str(), m, pos, object.at("size").get<float>());
            p->Luz(false); m_Malhas->insereUltimoLDDE(&p);
        }
    }
    PreparaCena();
    if (!scene.contains("bounds")) {
        vetor3 low(-14, -6, -6), high(14, 8, 30);
        for (int i = 0; i < GetNrPrimitivas(); ++i) {
            for (int axis = 0; axis < 3; ++axis) {
                float a, b;
                GetPrimitiva(i)->CalculaIntervalo(a, b, axis);
                low.cell[axis] = std::min(low.cell[axis], a);
                high.cell[axis] = std::max(high.cell[axis], b);
            }
        }
        vetor3 margin(.01f, .01f, .01f);
        m_Extends = aabb(low - margin, high - low + margin * 2.f);
    }

}
