#pragma once

#include <vector>
#include <cstddef>

namespace rfb { namespace cache {

template<typename T>
inline std::vector<std::vector<T>> batchForSending(const std::vector<T>& items,
                                                   size_t maxBatchSize = 100)
{
  std::vector<std::vector<T>> batches;
  if (items.empty()) return batches;
  for (size_t offset = 0; offset < items.size(); offset += maxBatchSize) {
    size_t end = std::min(offset + maxBatchSize, items.size());
    batches.emplace_back(items.begin() + offset, items.begin() + end);
  }
  return batches;
}

}} // namespace rfb::cache
