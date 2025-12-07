#!/usr/bin/env python3
"""Helper for detecting dark rectangle corruptions in screenshots.

Identifies solid dark rectangular regions that appear in one image but not
in another, which can indicate visual corruption artifacts from caching bugs.
"""

from __future__ import annotations

from dataclasses import dataclass
from pathlib import Path
from typing import List, Tuple

import numpy as np
from PIL import Image

from framework import PreflightError


@dataclass
class DarkRectangle:
    """A solid dark rectangular region."""
    
    x0: int
    y0: int
    x1: int  # inclusive
    y1: int  # inclusive
    
    @property
    def width(self) -> int:
        return self.x1 - self.x0 + 1
    
    @property
    def height(self) -> int:
        return self.y1 - self.y0 + 1
    
    @property
    def area(self) -> int:
        return self.width * self.height
    
    def __repr__(self) -> str:
        return f"DarkRectangle(pos=({self.x0},{self.y0}), size={self.width}×{self.height})"


@dataclass
class DarkRectDetectionResult:
    """Result of dark rectangle detection comparison."""
    
    image1_only: List[DarkRectangle]
    image2_only: List[DarkRectangle]
    
    @property
    def has_exclusive_rects(self) -> bool:
        """True if either image has dark rectangles the other doesn't."""
        return len(self.image1_only) > 0 or len(self.image2_only) > 0
    
    @property
    def total_exclusive_rects(self) -> int:
        return len(self.image1_only) + len(self.image2_only)


def find_dark_rectangles(
    image_path: Path,
    threshold: int = 40,
    min_width: int = 10,
) -> List[DarkRectangle]:
    """Find solid dark rectangular regions in an image.
    
    Args:
        image_path: Path to PNG image
        threshold: RGB threshold (all channels must be < threshold)
        min_width: Minimum width in pixels to be considered a rectangle
        
    Returns:
        List of DarkRectangle objects found
        
    Raises:
        PreflightError: if image cannot be loaded
    """
    if not image_path.is_file():
        raise PreflightError(f"Image not found: {image_path}")
    
    try:
        img = Image.open(image_path)
        arr = np.array(img)
    except Exception as exc:
        raise PreflightError(f"Failed to load image {image_path}: {exc}") from exc
    
    # Create mask of dark pixels (all RGB channels < threshold)
    if arr.ndim == 3 and arr.shape[2] >= 3:
        dark_mask = np.all(arr[:, :, :3] < threshold, axis=2)
    else:
        raise PreflightError(f"Unexpected image format: {arr.shape}")
    
    # Find horizontal runs of dark pixels
    runs = []
    for y in range(arr.shape[0]):
        row = dark_mask[y, :]
        if not np.any(row):
            continue
        
        # Find contiguous runs
        changes = np.diff(np.concatenate(([0], row.astype(int), [0])))
        starts = np.where(changes == 1)[0]
        ends = np.where(changes == -1)[0]
        
        for x0, x1 in zip(starts, ends):
            if x1 - x0 >= min_width:
                runs.append({'x0': x0, 'x1': x1 - 1, 'y': y})
    
    if not runs:
        return []
    
    # Merge consecutive rows with matching x ranges into rectangles
    rectangles = []
    
    # Sort by y, then x0
    runs.sort(key=lambda r: (r['y'], r['x0']))
    
    # Group runs by y coordinate
    from itertools import groupby
    y_groups = []
    for y, group in groupby(runs, key=lambda r: r['y']):
        y_groups.append((y, list(group)))
    
    # Merge consecutive y_groups where run structure matches
    i = 0
    while i < len(y_groups):
        y_start = y_groups[i][0]
        runs_at_y = y_groups[i][1]
        
        # Try to extend downwards (merge consecutive rows)
        y_end = y_start
        j = i + 1
        while j < len(y_groups):
            next_y = y_groups[j][0]
            next_runs = y_groups[j][1]
            
            # Check if next row is consecutive and has same run structure
            if next_y == y_end + 1 and len(next_runs) == len(runs_at_y):
                # Check if x ranges match (within 5px tolerance)
                matches = all(
                    abs(r1['x0'] - r2['x0']) <= 5 and abs(r1['x1'] - r2['x1']) <= 5
                    for r1, r2 in zip(runs_at_y, next_runs)
                )
                if matches:
                    y_end = next_y
                    j += 1
                else:
                    break
            else:
                break
        
        # Create rectangle(s) for this merged y range
        for run in runs_at_y:
            rectangles.append(DarkRectangle(
                x0=run['x0'],
                y0=y_start,
                x1=run['x1'],
                y1=y_end,
            ))
        
        i = j if j > i + 1 else i + 1
    
    return rectangles


def compare_dark_rectangles(
    image1_path: Path,
    image2_path: Path,
    threshold: int = 40,
    min_width: int = 10,
) -> DarkRectDetectionResult:
    """Compare two images for exclusive dark rectangles.
    
    Finds dark rectangles that appear in one image but not the other, which
    can indicate visual corruption artifacts.
    
    Args:
        image1_path: Path to first PNG image
        image2_path: Path to second PNG image
        threshold: RGB threshold for "dark" (all channels < threshold)
        min_width: Minimum width to be considered a rectangle
        
    Returns:
        DarkRectDetectionResult with rectangles exclusive to each image
        
    Raises:
        PreflightError: if images cannot be loaded or dimensions differ
    """
    # Load both images
    if not image1_path.is_file():
        raise PreflightError(f"Image not found: {image1_path}")
    if not image2_path.is_file():
        raise PreflightError(f"Image not found: {image2_path}")
    
    try:
        img1 = Image.open(image1_path)
        img2 = Image.open(image2_path)
        arr1 = np.array(img1)
        arr2 = np.array(img2)
    except Exception as exc:
        raise PreflightError(f"Failed to load images: {exc}") from exc
    
    if arr1.shape != arr2.shape:
        raise PreflightError(
            f"Image dimensions differ: {arr1.shape} vs {arr2.shape}"
        )
    
    # Create dark masks
    dark1 = np.all(arr1[:, :, :3] < threshold, axis=2)
    dark2 = np.all(arr2[:, :, :3] < threshold, axis=2)
    
    # Find pixels dark in one but not the other
    only_in_1 = dark1 & ~dark2
    only_in_2 = dark2 & ~dark1
    
    # Find rectangles in each exclusive mask
    def find_rects_in_mask(mask: np.ndarray) -> List[DarkRectangle]:
        runs = []
        for y in range(mask.shape[0]):
            row = mask[y, :]
            if not np.any(row):
                continue
            
            changes = np.diff(np.concatenate(([0], row.astype(int), [0])))
            starts = np.where(changes == 1)[0]
            ends = np.where(changes == -1)[0]
            
            for x0, x1 in zip(starts, ends):
                if x1 - x0 >= min_width:
                    runs.append({'x0': x0, 'x1': x1 - 1, 'y': y})
        
        if not runs:
            return []
        
        rectangles = []
        runs.sort(key=lambda r: (r['y'], r['x0']))
        
        from itertools import groupby
        y_groups = []
        for y, group in groupby(runs, key=lambda r: r['y']):
            y_groups.append((y, list(group)))
        
        i = 0
        while i < len(y_groups):
            y_start = y_groups[i][0]
            runs_at_y = y_groups[i][1]
            
            y_end = y_start
            j = i + 1
            while j < len(y_groups):
                next_y = y_groups[j][0]
                next_runs = y_groups[j][1]
                
                if next_y == y_end + 1 and len(next_runs) == len(runs_at_y):
                    matches = all(
                        abs(r1['x0'] - r2['x0']) <= 5 and abs(r1['x1'] - r2['x1']) <= 5
                        for r1, r2 in zip(runs_at_y, next_runs)
                    )
                    if matches:
                        y_end = next_y
                        j += 1
                    else:
                        break
                else:
                    break
            
            for run in runs_at_y:
                rectangles.append(DarkRectangle(
                    x0=run['x0'],
                    y0=y_start,
                    x1=run['x1'],
                    y1=y_end,
                ))
            
            i = j if j > i + 1 else i + 1
        
        return rectangles
    
    rects1 = find_rects_in_mask(only_in_1)
    rects2 = find_rects_in_mask(only_in_2)
    
    return DarkRectDetectionResult(
        image1_only=rects1,
        image2_only=rects2,
    )
