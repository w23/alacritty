#!/usr/bin/env python3

import sys
import argparse

from matplotlib import pyplot as plt
import numpy as np

parser = argparse.ArgumentParser(description='Process some integers')
parser.add_argument('input', metavar='file', type=argparse.FileType('rb'), nargs='+', help='Input files with raw integers')
args = parser.parse_args()

data = []

fig, ax = plt.subplots(1, 2, constrained_layout=True)

pcts = [50, 75, 90, 99]

for f in args.input:
    name = f.name
    a = np.fromfile(f, dtype=np.uint32)

    percentiles = np.percentile(a, pcts)
    hist_label = name + ' ('
    for i, pct in enumerate(pcts):
        hist_label += f'p{(100-pct):02}={int(percentiles[i])}us '
    hist_label += ')'

    ax[0].plot(a, label=name)
    ax[1].hist(a, label=hist_label, bins='auto')

ax[0].set(ylabel='time (us)', title='Rendering time')
ax[1].set(ylabel='time (us)', title='Rendering time')

ax[0].legend(title='measurements')
ax[1].legend(title='measurements')

#ax.grid()
#fig.savefig("test.png")
#plt.title("histogram")
plt.show()
