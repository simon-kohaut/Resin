import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

results = pd.read_csv("output/data/foc_estimation.csv")

fig, axes = plt.subplots(2, 1)

colors = [sns.color_palette("Paired", 10)[1], sns.color_palette("Paired", 10)[7], sns.color_palette("Paired", 10)[9]] 

results["Error"] = abs(results["True"] - results["Estimated"])
sns.lineplot(ax=axes[0], data=results[results["BinSize"] == 1], x="Measurement", y="Estimated", label="Estimated", color=colors[0]);
sns.lineplot(ax=axes[0], data=results[results["BinSize"] == 1], x="Measurement", y="True", label="True", color=colors[1]);
sns.despine(top=True, right=True)

axes[0].set_xlabel("#Measurements", fontsize=15)
axes[0].set_ylabel("FoC Tracking", fontsize=15)
axes[0].tick_params(labelsize=10)
# axes[0].tick_params(
#     axis='x',          # changes apply to the x-axis
#     which='both',      # both major and minor ticks are affected
#     bottom=False,      # ticks along the bottom edge are off
#     top=False,         # ticks along the top edge are off
#     labelbottom=False) # labels along the bottom edge are off
# axes[0].set_xlabel("")
# axes[0].get_legend().remove()
axes[0].set_ylim([0, 30])
axes[0].set_xlim([0, 1200])
axes[0].legend(loc="upper right")
# plot.set_yticks([0, 2.5, 5])

results["ErrorCluster"] = abs(results["TrueCluster"] - results["EstimatedCluster"])
sns.barplot(ax=axes[1], data=results, x="BinSize", y="ErrorCluster", color=colors[2]);

axes[1].tick_params(labelsize=10)
axes[1].set_xlabel("Partitioning", fontsize=15)
axes[1].set_ylabel("Partition MAE", fontsize=15)
axes[1].tick_params(labelsize=10)
axes[1].set_xticks(ticks=range(10), labels=[f"{h}Hz" for h in range(1, 11)])

fig.align_ylabels(axes)
fig.tight_layout()
fig.savefig("output/plots/foc_estimation.pdf", bbox_inches='tight')
