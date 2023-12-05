import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

original_results = pd.read_csv("output/data/original_inference_times.csv", low_memory=False)
adapted_results = pd.read_csv("output/data/adapted_inference_times.csv", low_memory=False)

# bars = pd.DataFrame(columns=["BinSize", "Location", "Runtime"])
# for bin_size in [0.5, 1.0, 1.5, 2.5, 5.0, 7.5, 10.0]:
#     for location in [10]:
#         subset = results[results["BinSize"] == bin_size]
#         subset = subset[subset["Location"] == location]
#         subset = subset[subset["Time"] < 20]

#         data = pd.DataFrame(data={"BinSize": bin_size, "Location": location, "Adapted": False, "Runtime": subset["Runtime"].mean()}, index=[0])
#         bars = pd.concat([bars, data], ignore_index=True)

#         subset = results[results["BinSize"] == bin_size]
#         subset = subset[subset["Location"] == location]
#         subset = subset[subset["Time"] > 20]

#         data = pd.DataFrame(data={"BinSize": bin_size, "Location": location, "Adapted": True, "Runtime": subset["Runtime"].mean()}, index=[0])
#         bars = pd.concat([bars, data], ignore_index=True)

# plot = sns.lineplot(data=original_results, x="BinSize", y="Runtime", label="Original")
fig, axes = plt.subplots(2, 1)

sns.barplot(ax=axes[0], data=adapted_results, x="BinSize", y="Runtime", hue="Location", palette=sns.color_palette(), errorbar=None)
sns.lineplot(ax=axes[1], data=adapted_results, x="BinSize", y="Depth", hue="Location", palette=sns.color_palette())

baseline = original_results["Runtime"].mean()
axes[0].axhline(baseline, ls='--', c="r")
axes[0].text(1, baseline + 0.075 * baseline, "Baseline")

axes[0].set_ylabel("Runtime / s", fontsize=20)
axes[0].tick_params(labelsize=15)
axes[0].set_yscale("log")
axes[0].get_legend().remove()
axes[0].tick_params(
    axis='x',          # changes apply to the x-axis
    which='both',      # both major and minor ticks are affected
    bottom=False,      # ticks along the bottom edge are off
    top=False,         # ticks along the top edge are off
    labelbottom=False) # labels along the bottom edge are off
axes[0].set_xlabel("")


axes[1].set_xlabel("Partitioning", fontsize=20)
axes[1].set_ylabel(r"$\mathcal{RC}$ Depth", fontsize=20)
axes[1].tick_params(labelsize=15)
axes[1].set_yticks(range(1, 10, 2))
axes[1].set_xticks(range(1, 11));
axes[1].set_xticklabels(["1Hz", "2Hz", "3Hz", "4Hz", "5Hz", "6Hz", "7Hz", "8Hz", "9Hz", "10Hz"])
h, l = axes[1].get_legend_handles_labels()
labels = [r"$\mathcal{N}(" + label + r", 2)$" for label in l]
axes[1].legend(h, labels, title="FoC PDF")

sns.despine(top=True, right=True)
fig.tight_layout()
fig.savefig("output/plots/time_plot.pdf", bbox_inches='tight')
