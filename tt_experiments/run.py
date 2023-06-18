#!/usr/bin/env python3
import random


def run_table(size: int, num_entries: int) -> int:
    """
    size 個のバケット、1 バケット内にエントリーが num_entries 個あるとき、
    衝突 (あるバケットの中で num_entries 個より多く入ること) が起こるまでの成功した挿入回数を返す。
    """
    table = [[] for _ in range(size)]
    count = 0
    while True:
        hash = random.getrandbits(64)
        index = hash % size
        if hash not in table[index]:
            table[index].append(hash)
            if len(table[index]) > num_entries:
                break
        count += 1
    return count


sizes = [1 << 16, 1 << 17, 1 << 18, 1 << 19, 1 << 20]
nums_entries = [1, 2, 3, 4]
for size in sizes:
    for num_entries in nums_entries:
        print(size, num_entries, run_table(size, num_entries))
