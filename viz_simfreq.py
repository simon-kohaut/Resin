import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

###################
# Import
data = pd.read_csv("output/data/simulation_frequencies_results.csv", low_memory=False)

###################
# Setup
colors = [sns.color_palette("Paired", 10)[1], sns.color_palette("Paired", 10)[7], sns.color_palette("Paired", 10)[9]] 

fig, axes = plt.subplots(7, 7, sharex=True, sharey=True)

###################
# How many leafs are in layer 1
# data = pd.melt(data, ["Time", "f0", "f2", "f4", "f6", "f8", "f10", "f12", "f14", "f16", "f18", "f20", "f22", "f24", "f26", "f28", "f30", "f32", "f34", "f36", "f38", "f40"])

index = 0
for i in range(7):
    for j in range(7):
        if i == j or i < j:
            fig.delaxes(axes[i, j])
            continue

        sns.lineplot(data[data["Time"] <= 60.0], ax=axes[i, j], x="Time", y=f"f{index}")
        axes[i, j].set_ylim([1, 200])
        axes[i, j].set_xlim([0, 60])
        axes[i, j].set_yticks([0, 200])
        axes[i, j].set_xticks([0, 60])
        axes[i, j].set_ylabel("")
        axes[i, j].set_xlabel("")

        index += 2

###################
# Export
sns.despine(top=True, right=True)
fig.supxlabel("Simulation Time / s", fontsize=15, fontname="Times New Roman")
fig.supylabel("Frequency of Change / Hz", fontsize=15, fontname="Times New Roman")
fig.subplots_adjust(hspace=0.5, wspace=0.5)
fig.savefig("output/plots/sim_frequencies.pdf", bbox_inches='tight')

###################
# Show when frequency is consideres layer 1 (dynamic) vs layer 2 (static)

fig, axes = plt.subplots(7, 7, sharex=True, sharey=True)
for index in range(42):
    data[f"f{index}"][data[f"f{index}"] >= 1.0] = 1
    data[f"f{index}"][data[f"f{index}"] < 1.0] = 0

index = 0
for i in range(7):
    for j in range(7):
        if i == j or i < j:
            fig.delaxes(axes[i, j])
            continue

        sns.lineplot(data[data["Time"] <= 60.0], ax=axes[i, j], x="Time", y=f"f{index}")
        axes[i, j].set_ylim([0, 1])
        axes[i, j].set_yticks([0, 1])
        axes[i, j].set_xlim([0, 60])
        axes[i, j].set_xticks([0, 60])
        axes[i, j].set_ylabel("")
        axes[i, j].set_xlabel("")

        index += 2

###################
# Export
sns.despine(top=True, right=True)
fig.supxlabel("Simulation Time / s", fontsize=15, fontname="Times New Roman")
fig.supylabel("Partition", fontsize=15, fontname="Times New Roman")
fig.subplots_adjust(hspace=0.5, wspace=0.5)
fig.savefig("output/plots/sim_dynamic.pdf", bbox_inches='tight')