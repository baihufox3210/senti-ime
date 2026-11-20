#pragma once
#include <json-c/json.h>
#include <memory>
#include <stdexcept>
#include <string>

namespace senti {
using Json = std::shared_ptr<json_object>;
inline Json own(json_object *value) {
  return Json(value, [](json_object *v) {
    if (v)
      json_object_put(v);
  });
}
inline Json object() { return own(json_object_new_object()); }
inline Json string(const std::string &value) {
  return own(
      json_object_new_string_len(value.data(), static_cast<int>(value.size())));
}
inline Json number(int64_t value) { return own(json_object_new_int64(value)); }
inline Json boolean(bool value) { return own(json_object_new_boolean(value)); }
inline void set(const Json &parent, const char *key, const Json &value) {
  json_object_object_add(parent.get(), key, json_object_get(value.get()));
}
inline Json get(const Json &parent, const char *key) {
  json_object *value = nullptr;
  if (!parent || !json_object_object_get_ex(parent.get(), key, &value))
    return {};
  return own(json_object_get(value));
}
inline std::string text(const Json &value, const std::string &fallback = {}) {
  if (!value || !json_object_is_type(value.get(), json_type_string))
    return fallback;
  return std::string(json_object_get_string(value.get()),
                     json_object_get_string_len(value.get()));
}
inline int64_t integer(const Json &value, int64_t fallback = 0) {
  return value && json_object_is_type(value.get(), json_type_int)
             ? json_object_get_int64(value.get())
             : fallback;
}
inline bool truth(const Json &value) {
  return value && json_object_is_type(value.get(), json_type_boolean) &&
         json_object_get_boolean(value.get());
}
inline Json command(const char *type, const std::string &context = {}) {
  auto value = object();
  set(value, "type", string(type));
  if (!context.empty())
    set(value, "context", string(context));
  return value;
}
} // namespace senti
