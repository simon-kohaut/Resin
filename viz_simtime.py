import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

###################
# Import
data = pd.read_csv("output/data/simulation_results.csv", low_memory=False)

###################
# Setup
colors = [sns.color_palette("Paired", 10)[1], sns.color_palette("Paired", 10)[7], sns.color_palette("Paired", 10)[9]] 

fig, axes = plt.subplots(2, 1)

###################
# Inference time
time_data = data[["Time", "OriginalRuntime", "AdaptedRuntime", "AdaptedFullRuntime"]]
time_data = time_data.rename(columns={"OriginalRuntime": "Flat", "AdaptedFullRuntime": "Adapted", "AdaptedRuntime": "Reactive"})
time_data = pd.melt(time_data, ["Time"])

sns.lineplot(time_data[time_data["Time"] <= 60.0], ax=axes[0], x="Time", y="value", hue="variable", palette=colors, linewidth=0.75)
axes[0].set_xlim([0, 60])
# axes[0].set_aspect(3.0)
axes[0].tick_params(labelsize=10)
axes[0].set_yscale("log")
axes[0].set_ylabel("Runtime / s", fontsize=15, fontname="Times New Roman")
axes[0].set_xlabel("")

# get the legend object
leg = axes[0].legend(loc="upper right")

# change the line width for the legend
for line in leg.get_lines():
    line.set_linewidth(1.0)

sns.lineplot(time_data, ax=axes[1], x="Time", y="value", hue="variable", palette=colors, linewidth=0.5)
# axes[1].set_aspect(75.0)
axes[1].set_xlim([0, 2100])
axes[1].set_xticks([0, 600, 1200, 1800])
axes[1].tick_params(labelsize=10)
axes[1].tick_params(
    axis='y',          # changes apply to the x-axis
    which='both',      # both major and minor ticks are affected
    left=False,      # ticks along the bottom edge are off
    right=False)         # ticks along the top edge are off
axes[1].set_ylabel("Runtime / s", fontsize=15, fontname="Times New Roman")
axes[1].set_yscale("log")
axes[1].set_xlabel("Simulation Time / s", fontsize=15, fontname="Times New Roman")
axes[1].legend(title="Circuit")
axes[1].get_legend().remove()

###################
# Export
sns.despine(top=True, right=True)
fig.tight_layout()
fig.savefig("output/plots/sim_plot.pdf", bbox_inches='tight')
