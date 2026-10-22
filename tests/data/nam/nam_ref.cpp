// Reference renderer: run a .nam model (NeuralAmpModelerCore) on raw float32 input.
//   nam_ref <model.nam> <in.f32> <out.f32> <block size>
#include <cstdio>
#include <filesystem>
#include <fstream>
#include <vector>
#include "NAM/dsp.h"
#include "NAM/get_dsp.h"

int main(int argc, char** argv) {
  if (argc != 5) { fprintf(stderr, "usage: nam_ref model in out block\n"); return 2; }
  auto model = nam::get_dsp(std::filesystem::path(argv[1]));
  const int block = atoi(argv[4]);
  model->ResetAndPrewarm(48000.0, block);
  std::ifstream in2(argv[2], std::ios::binary | std::ios::ate);
  const size_t n = in2.tellg() / sizeof(float);
  in2.seekg(0);
  std::vector<float> buf(n), out(n);
  in2.read(reinterpret_cast<char*>(buf.data()), n * sizeof(float));
  for (size_t pos = 0; pos < n; pos += block) {
    const int m = (int)std::min<size_t>(block, n - pos);
    float* ip = buf.data() + pos;
    float* op = out.data() + pos;
    model->process(&ip, &op, m);
  }
  std::ofstream o(argv[3], std::ios::binary);
  o.write(reinterpret_cast<const char*>(out.data()), n * sizeof(float));
  fprintf(stderr, "%s: %zu samples\n", argv[1], n);
  return 0;
}
