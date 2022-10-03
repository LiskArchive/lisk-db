#!/bin/sh

helpFunction()
{
   echo ""
   echo "Usage: $0 benchmark_name"
   exit 1 # Exit script after printing help
}

# Print helpFunction in case a parameter is missing
if [ -z "$1" ]
then
   echo "Benchmark name is missing";
   helpFunction
fi

# Begin script in case of benchmark name is present
benchmark_name=$1

echo "Executing benchmark for: $benchmark_name\n"

if ! cargo +1.59 build --profile=release-with-debug --manifest-path="../../Cargo.toml" --bin $benchmark_name ; then
    echo "Failed to build benchmark"
    exit 1
fi

echo "Benchmark with Valgrind using Callgrind profiler:"
if valgrind --tool=callgrind --dump-instr=yes --collect-jumps=yes --simulate-cache=yes --callgrind-out-file=callgrind.out ../../target/release-with-debug/$benchmark_name; then
    if [[ "$OSTYPE" == "darwin"* ]]; then
        qcachegrind callgrind.out &
    else
        kcachegrind callgrind.out &
    fi
    
fi
