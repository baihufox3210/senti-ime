// Score the decoder's finite candidates using local Qwen continuation logits.
#include <ggml-backend.h>
#include <json-c/json.h>
#include <llama.h>

#include <algorithm>
#include <array>
#include <cmath>
#include <iostream>
#include <memory>
#include <string>
#include <string_view>
#include <vector>

namespace {
using Json = std::unique_ptr<json_object, decltype(&json_object_put)>;
constexpr size_t maxOptions = 6;
constexpr int batchSize = 256;

std::vector<llama_token> tokenize(const llama_vocab *vocab,
                                 std::string_view text) {
  std::vector<llama_token> tokens(text.size() + 4);
  auto count = llama_tokenize(vocab, text.data(), text.size(), tokens.data(),
                              tokens.size(), false, false);
  if (count < 0)
    return {};
  tokens.resize(count);
  return tokens;
}

std::string_view stringAt(json_object *array, size_t index) {
  auto *item = json_object_array_get_idx(array, index);
  if (!json_object_is_type(item, json_type_string))
    return {};
  return {json_object_get_string(item),
          static_cast<size_t>(json_object_get_string_len(item))};
}

struct Batch {
  llama_batch data = llama_batch_init(batchSize, 0, maxOptions);
  ~Batch() { llama_batch_free(data); }

  int add(llama_token token, int position, int sequence, bool logits) {
    const auto row = data.n_tokens++;
    data.token[row] = token;
    data.pos[row] = position;
    data.n_seq_id[row] = sequence < 0 ? maxOptions : 1;
    for (int i = 0; i < data.n_seq_id[row]; ++i)
      data.seq_id[row][i] = sequence < 0 ? i : sequence;
    data.logits[row] = logits;
    return row;
  }
};

double logPartition(const float *logits, int vocabSize) {
  const auto max = *std::max_element(logits, logits + vocabSize);
  double sum = 0;
  for (int i = 0; i < vocabSize; ++i) {
    const auto gap = logits[i] - max;
    if (gap >= -16)
      sum += std::exp(gap);
  }
  return max + std::log(sum);
}

Json score(llama_context *ctx, const llama_vocab *vocab, json_object *request) {
  Json none(nullptr, json_object_put);
  if (!json_object_is_type(request, json_type_array) ||
      json_object_array_length(request) != 3)
    return none;
  auto *options = json_object_array_get_idx(request, 2);
  if (!json_object_is_type(options, json_type_array))
    return none;
  const auto count = json_object_array_length(options);
  if (count < 2 || count > maxOptions)
    return none;

  auto prefix = tokenize(vocab, stringAt(request, 0));
  if (prefix.empty())
    prefix = tokenize(vocab, "。");
  if (prefix.size() > 96)
    prefix.erase(prefix.begin(), prefix.end() - 96);
  auto right = tokenize(vocab, stringAt(request, 1));
  if (right.size() > 3)
    right.resize(3);

  struct Path {
    std::vector<llama_token> tokens;
    size_t wordTokens;
    std::vector<int> precedingRows;
  };
  std::array<Path, maxOptions> paths{};
  Batch batch;
  for (size_t i = 0; i < prefix.size(); ++i)
    batch.add(prefix[i], i, -1, i + 1 == prefix.size());
  for (size_t i = 0; i < count; ++i) {
    auto word = tokenize(vocab, stringAt(options, i));
    if (word.empty() || word.size() > 16 ||
        batch.data.n_tokens + word.size() + right.size() > batchSize)
      return none;
    auto &path = paths[i];
    path.wordTokens = word.size();
    path.tokens = std::move(word);
    path.tokens.insert(path.tokens.end(), right.begin(), right.end());
    path.precedingRows.push_back(prefix.size() - 1);
    for (size_t j = 0; j + 1 < path.tokens.size(); ++j)
      path.precedingRows.push_back(
          batch.add(path.tokens[j], prefix.size() + j, i, true));
  }
  llama_memory_clear(llama_get_memory(ctx), false);
  if (llama_decode(ctx, batch.data) != 0)
    return none;

  const auto vocabSize = llama_vocab_n_tokens(vocab);
  const auto *sharedLogits = llama_get_logits_ith(ctx, prefix.size() - 1);
  if (!sharedLogits)
    return none;
  const auto sharedPartition = logPartition(sharedLogits, vocabSize);
  Json result(json_object_new_array_ext(count), json_object_put);
  for (size_t i = 0; i < count; ++i) {
    const auto &path = paths[i];
    double value = 0;
    for (size_t j = 0; j < path.tokens.size(); ++j) {
      const auto *logits = llama_get_logits_ith(ctx, path.precedingRows[j]);
      if (!logits)
        return none;
      const auto partition = j == 0 ? sharedPartition : logPartition(logits, vocabSize);
      const double weight = j < path.wordTokens ? 1.0 : 0.75;
      value += weight * (logits[path.tokens[j]] - partition);
    }
    if (!std::isfinite(value))
      return none;
    json_object_array_add(result.get(), json_object_new_double(value));
  }
  return result;
}
} // namespace

int main(int argc, char **argv) {
  if (argc < 3 || argc > 4)
    return 2;
  ggml_backend_load_all_from_path(argv[2]);
  llama_backend_init();
  auto modelParams = llama_model_default_params();
  const bool cpu = argc == 4 && std::string_view(argv[3]) == "cpu";
  modelParams.n_gpu_layers = cpu ? 0 : -1;
  const auto defaultLoadMode = modelParams.load_mode;
  if (!cpu && ggml_backend_dev_by_type(GGML_BACKEND_DEVICE_TYPE_GPU)) {
    // Upload the weights without retaining the GGUF mapping on the host.
    modelParams.load_mode = LLAMA_LOAD_MODE_NONE;
  }
  std::unique_ptr<llama_model, decltype(&llama_model_free)> model(
      llama_model_load_from_file(argv[1], modelParams), llama_model_free);
  if (!model && modelParams.load_mode != defaultLoadMode) {
    modelParams.load_mode = defaultLoadMode;
    model.reset(llama_model_load_from_file(argv[1], modelParams));
  }
  if (!model)
    return 1;

  auto params = llama_context_default_params();
  // Each request clears the unified KV cache. Its shared prefix and all
  // candidate paths already fit within one batch.
  params.n_ctx = batchSize;
  params.n_batch = batchSize;
  params.n_ubatch = batchSize;
  params.n_outputs_max = 64;
  params.n_seq_max = maxOptions;
  params.kv_unified = true;
  params.n_threads = 2;
  params.n_threads_batch = 4;
  std::unique_ptr<llama_context, decltype(&llama_free)> ctx(
      llama_init_from_model(model.get(), params), llama_free);
  if (!ctx)
    return 1;

  const auto *vocab = llama_model_get_vocab(model.get());
  Json warmup(json_tokener_parse("[\"請參考\",\"執行\",[\"指示\",\"只是\"]]"),
              json_object_put);
  if (!score(ctx.get(), vocab, warmup.get()))
    return 1;
  std::cout << "ready" << std::endl;
  std::string line;
  while (std::getline(std::cin, line)) {
    Json request(json_tokener_parse(line.c_str()), json_object_put);
    auto result = score(ctx.get(), vocab, request.get());
    std::cout << (result ? json_object_to_json_string_ext(result.get(), JSON_C_TO_STRING_PLAIN)
                         : "null")
              << std::endl;
  }
}
