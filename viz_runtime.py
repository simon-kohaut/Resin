import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

###################
# Import
original_results = pd.read_csv("output/data/original_inference_times.csv", low_memory=False)
adapted_results = pd.read_csv("output/data/adapted_inference_times.csv", low_memory=False)

fig, axes = plt.subplots(3, 1)
colors = [sns.color_palette("Paired", 10)[1], sns.color_palette("Paired", 10)[7], sns.color_palette("Paired", 10)[9]] 

###################
# Speedup
adapted_results["Speedup"] = original_results["Runtime"].mean() / adapted_results["Runtime"]
sns.barplot(ax=axes[0], data=adapted_results, x="BinSize", y="Speedup", hue="Location", palette=colors, errorbar=None)

axes[0].set_ylabel("Speedup", fontsize=15, fontname="Times New Roman")
axes[0].tick_params(labelsize=10)
axes[0].set_yscale("log")
axes[0].tick_params(
    axis='x',          # changes apply to the x-axis
    which='both',      # both major and minor ticks are affected
    bottom=False,      # ticks along the bottom edge are off
    top=False,         # ticks along the top edge are off
    labelbottom=False) # labels along the bottom edge are off
axes[0].set_xlabel("")
axes[0].get_legend().remove()

###################
# Size
adapted_results["SizeIncrease"] = adapted_results["Size"] / original_results["Size"].mean()
sns.barplot(ax=axes[1], data=adapted_results, x="BinSize", y="SizeIncrease", hue="Location", palette=colors, errorbar=None)

axes[1].set_ylabel("Mem. Ratio", fontsize=15, fontname="Times New Roman")
axes[1].tick_params(labelsize=10)
axes[1].tick_params(
    axis='x',          # changes apply to the x-axis
    which='both',      # both major and minor ticks are affected
    bottom=False,      # ticks along the bottom edge are off
    top=False,         # ticks along the top edge are off
    labelbottom=False) # labels along the bottom edge are off
axes[1].set_xlabel("")
axes[1].set_yticks(ticks=range(0, 16, 5))

h, l = axes[1].get_legend_handles_labels()
labels = [r"$\mathcal{N}(" + label + r", 1)$" for label in l]
axes[1].legend(h, labels, title="FoC PDF")

###################
# Depth
sns.barplot(ax=axes[2], data=adapted_results, x="BinSize", y="Depth", hue="Location", palette=colors, errorbar=None)
axes[2].set_xlabel("Partitioning", fontsize=15, fontname="Times New Roman")
axes[2].set_ylabel("Depth", fontsize=15, fontname="Times New Roman")
axes[2].set_ylim([1, 10])
axes[2].set_yticks(range(1, 11, 3))
axes[2].tick_params(labelsize=10)
axes[2].get_legend().remove()
axes[2].set_xticks(ticks=range(10), labels=[f"{h}Hz" for h in range(1, 11)])

###################
# Export
sns.despine(top=True, right=True)
fig.tight_layout()
fig.align_ylabels(axes)
fig.savefig("output/plots/time_plot.pdf", bbox_inches='tight')
