import seaborn as sns
import pandas as pd
import matplotlib.pyplot as plt

###################
# Import
data = pd.read_csv("output/data/simulation_frequencies_results.csv", low_memory=False)

###################
# Setup
colors = [sns.color_palette("Paired", 10)[1], sns.color_palette("Paired", 10)[7], sns.color_palette("Paired", 10)[9]] 

###################
# Inference time
data = data.melt(id_vars="Time")
sns.lineplot(data, x="Time", y="value", hue="variable")

###################
# Export
sns.despine(top=True, right=True)

plt.tick_params(labelsize=10)
plt.yscale("log")
plt.ylabel("Souce Freq. / Hz", fontsize=15, fontname="Times New Roman")
plt.xlabel("Simulation Time / s", fontsize=15, fontname="Times New Roman")
plt.legend().remove()
plt.tight_layout()

plt.savefig("output/plots/sim_freq_plot.pdf", bbox_inches='tight')
