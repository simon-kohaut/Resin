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
plot = sns.barplot(data=original_results, x=0, y="Runtime", label="Original", errorbar=None)

# for location in [1.0, 5.0, 10.0]:
#     distribution_results = adapted_results[adapted_results["Location"] == location]
plot = sns.barplot(data=adapted_results, x="BinSize", y="Runtime", hue="Location", palette=sns.color_palette()[1:], errorbar=None)

h, l = plot.get_legend_handles_labels()
labels = [r'$\mathcal{N}(' + f"{label}" + r', 2)$' for label in l if label != "Original"]
plot.legend(h, labels + ["Original"])

plt.gcf().tight_layout()
sns.despine(top=True, right=True)

plot.set_xlabel("Partitioning", fontsize=20)
plot.set_ylabel("Inference Time / s", fontsize=20)
plot.tick_params(labelsize=15)
plot.set_yscale("log")
plot.set_aspect(10.0 / 7.0)
plot.set_xticks([0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10])
plot.set_xticklabels(["None", "1Hz", "2Hz", "3Hz", "4Hz", "5Hz", "6Hz", "7Hz", "8Hz", "9Hz", "10Hz"])

plot.get_figure().savefig("output/plots/time_plot.pdf", bbox_inches='tight')
