// extern "C" wrappers around the C++ style parser and the background-layer
// paint property evaluation, for //difftest/style.
//
// The evaluation mirrors RenderOrchestrator + RenderBackgroundLayer:
// untransitioned(), transitioned(TransitionParameters{now, options}),
// ZoomHistory::update(zoom, now), evaluate(PropertyEvaluationParameters).

#include <mln/map/zoom_history.hpp>
#include <mln/renderer/property_evaluation_parameters.hpp>
#include <mln/renderer/property_evaluator.hpp>
#include <mln/renderer/transition_parameters.hpp>
#include <mln/style/layer_impl.hpp>
#include <mln/style/layers/background_layer.hpp>
#include <mln/style/layers/background_layer_impl.hpp>
#include <mln/style/layers/background_layer_properties.hpp>
#include <mln/style/parser.hpp>
#include <mln/style/properties.hpp>
#include <mln/util/chrono.hpp>
#include <mln/util/color.hpp>

#include <cstddef>
#include <cstdint>
#include <cstring>
#include <string>

using namespace mln;
using namespace mln::style;

extern "C" {

struct MlnStyleRefBackground {
    float color[4];
    float opacity;
    int32_t visibility; // 0 = visible, 1 = none
    int32_t pattern_empty;
    int32_t layer_count;
    double zoom;
};

// Returns 0 on success, -1 parse error, -2 not a background layer,
// -3 no such layer, -4 exception.
int32_t mln_style_ref_evaluate_background(const char* json,
                                          size_t json_len,
                                          const char* layer_id,
                                          size_t layer_id_len,
                                          int64_t now_ns,
                                          MlnStyleRefBackground* out) {
    try {
        Parser parser;
        if (auto error = parser.parse(std::string(json, json_len))) {
            return -1;
        }
        const std::string id(layer_id, layer_id_len);
        out->layer_count = static_cast<int32_t>(parser.layers.size());
        out->zoom = parser.zoom;
        for (const auto& layer : parser.layers) {
            if (layer->getID() != id) continue;
            if (layer->getTypeInfo() != BackgroundLayer::Impl::staticTypeInfo()) return -2;
            const auto& impl = static_cast<const BackgroundLayer::Impl&>(*layer->baseImpl);

            const TimePoint now{Duration(now_ns)};
            auto unevaluated = impl.paint.untransitioned();
            unevaluated = impl.paint.transitioned(TransitionParameters{now, TransitionOptions()},
                                                  std::move(unevaluated));
            ZoomHistory zoomHistory;
            zoomHistory.update(static_cast<float>(parser.zoom), now);
            const PropertyEvaluationParameters parameters{zoomHistory, now, Duration::zero()};
            const auto evaluated = unevaluated.evaluate(parameters);

            const Color color = evaluated.get<BackgroundColor>();
            out->color[0] = color.r;
            out->color[1] = color.g;
            out->color[2] = color.b;
            out->color[3] = color.a;
            out->opacity = evaluated.get<BackgroundOpacity>();
            const auto& pattern = evaluated.get<BackgroundPattern>();
            out->pattern_empty = pattern.from.empty() && pattern.to.empty();
            out->visibility = impl.visibility == VisibilityType::None ? 1 : 0;
            return 0;
        }
        return -3;
    } catch (...) {
        return -4;
    }
}

// Transitioning<PropertyValue<Color>> from `from` to `to` (constants) with
// the given options, created at t = 0 and evaluated at `elapsed_ns`.
void mln_style_ref_transition_color(const float* from,
                                    const float* to,
                                    int64_t duration_ms,
                                    int64_t delay_ms,
                                    int64_t elapsed_ns,
                                    float* out) {
    TransitionOptions options;
    if (duration_ms >= 0) options.duration = {Milliseconds(duration_ms)};
    if (delay_ms >= 0) options.delay = {Milliseconds(delay_ms)};
    const TimePoint start{Duration::zero()};
    Transitioning<PropertyValue<Color>> t0{
        PropertyValue<Color>(Color{from[0], from[1], from[2], from[3]}),
        Transitioning<PropertyValue<Color>>(),
        TransitionOptions(),
        start};
    Transitioning<PropertyValue<Color>> t1{
        PropertyValue<Color>(Color{to[0], to[1], to[2], to[3]}), t0, options, start};
    const PropertyEvaluationParameters parameters{0.0f};
    const PropertyEvaluator<Color> evaluator{parameters, Color::black()};
    const Color result = t1.evaluate(evaluator, start + Duration(elapsed_ns));
    out[0] = result.r;
    out[1] = result.g;
    out[2] = result.b;
    out[3] = result.a;
}

} // extern "C"
